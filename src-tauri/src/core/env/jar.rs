//! 第 1~2 层离线侧：包内 / 本地 jar 的元数据探测（自证端 + 自报身份 + 字节 sha1）。

use std::collections::HashMap;
use std::fs::File;
use std::io::{Cursor, Read};
use std::path::Path;

use sha1::{Digest, Sha1};

use crate::models::{EnvSource, SideFlag};
use super::code::{read_code_facts, CodeFacts};
use super::evidence::Evidence;

/// 单次扫描的模组数上限（超大包按序截断，剩下的走平台层或留未判定）
const JAR_MAX_FILES: usize = 400;
/// 读元数据所需的解压上限：只为取 fabric.mod.json / mods.toml，超大 jar 跳过元数据（哈希照算）
const JAR_MAX_UNCOMPRESSED: u64 = 48 * 1024 * 1024;
/// 并行读的线程数（每条一个 zip 句柄，中央目录各解析一次）
const JAR_THREADS: usize = 4;
/// 模组自证端 / 自报身份的元数据文件名
const JAR_META: &[&str] = &[
    "fabric.mod.json",
    "quilt.mod.json",
    "META-INF/mods.toml",
    "META-INF/neoforge.mods.toml",
];

/// 一个包内 jar 的探测结果：自证端 + 平台反查用的身份线索
#[derive(Clone, Debug, Default)]
pub struct JarProbe {
    /// Fabric / Quilt 的 `environment` 自证；Forge / NeoForge 无此字段，故常为 None
    pub env: Option<Evidence>,
    /// jar 原始字节的 sha1——与 Modrinth `files[].hashes.sha1` 同口径，可直接反查构建
    pub sha1: Option<String>,
    /// 模组 id（`fabric.mod.json:id` / `mods.toml:modId`），多半就是 Modrinth slug
    pub mod_id: Option<String>,
    /// 模组显示名（作者写的英文名），按名搜索兜底用
    pub title: Option<String>,
    /// 字节码结构提示（只在加载器元数据没自证端时才扫，见 `CodeFacts`）
    pub code: CodeFacts,
}

/// 一条离线探测请求。`want_sha1` 关掉时不整包算哈希——mrpack 已在 index 里给了 sha1，
/// 再算一遍等于白读几百 MB；裸 zip 与手动塞入的 jar 才需要我们自己算（第 3 层的入场券）。
/// `want_code` 同理关掉「逐个解 class」这条最贵的离线路（本地索引已答上的行不必解）
#[derive(Clone, Debug)]
pub struct ProbeReq {
    pub path: String,
    pub want_sha1: bool,
    pub want_code: bool,
}

/// 离线扫描源包内指定 jar 条目。读不到一律静默跳过（交上下层）。
pub fn probe_jars(pack: &Path, want: &[ProbeReq]) -> HashMap<String, JarProbe> {
    let mut out = HashMap::new();
    let want_map: HashMap<&str, (bool, bool)> = want
        .iter()
        .map(|w| (w.path.as_str(), (w.want_sha1, w.want_code)))
        .collect();
    let Ok(file) = File::open(pack) else {
        return out;
    };
    let Ok(archive) = zip::ZipArchive::new(file) else {
        return out;
    };
    let targets: Vec<(usize, String, bool, bool)> = archive
        .file_names()
        .enumerate()
        .filter_map(|(i, name)| {
            want_map
                .get(name)
                .map(|(hash, code)| (i, name.to_string(), *hash, *code))
        })
        .take(JAR_MAX_FILES)
        .collect();
    if targets.is_empty() {
        return out;
    }

    let per = targets.len().div_ceil(JAR_THREADS).max(1);
    let mut results: Vec<Vec<(String, JarProbe)>> = Vec::new();
    std::thread::scope(|s| {
        let handles: Vec<_> = targets
            .chunks(per)
            .map(|chunk| {
                let pack = pack.to_path_buf();
                let chunk: Vec<(usize, String, bool, bool)> = chunk.to_vec();
                s.spawn(move || jar_chunk(&pack, &chunk))
            })
            .collect();
        for h in handles {
            if let Ok(r) = h.join() {
                results.push(r);
            }
        }
    });
    for chunk in results {
        for (path, probe) in chunk {
            out.insert(path, probe);
        }
    }
    out
}

/// 一个分片：重开包句柄，逐个 jar 探测（线程内串行，跨线程并行）
fn jar_chunk(pack: &Path, chunk: &[(usize, String, bool, bool)]) -> Vec<(String, JarProbe)> {
    let mut out = Vec::new();
    let Ok(file) = File::open(pack) else {
        return out;
    };
    let Ok(mut archive) = zip::ZipArchive::new(file) else {
        return out;
    };
    for (idx, path, want_sha1, want_code) in chunk {
        if let Some(probe) = jar_probe(&mut archive, *idx, *want_sha1, *want_code) {
            out.push((path.clone(), probe));
        }
    }
    out
}

fn jar_probe<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    idx: usize,
    want_sha1: bool,
    want_code: bool,
) -> Option<JarProbe> {
    let mut entry = archive.by_index(idx).ok()?;
    let mut probe = JarProbe::default();
    if entry.size() <= JAR_MAX_UNCOMPRESSED {
        let mut buf = Vec::with_capacity(entry.size() as usize);
        if entry.read_to_end(&mut buf).is_err() {
            return None;
        }
        if want_sha1 {
            probe.sha1 = Some(sha1_hex(&buf));
        }
        read_meta(&buf, &mut probe);
        // 加载器元数据已自证端就别再解 class 了：Fabric/Quilt 的 `environment` 更强也更便宜
        if want_code && probe.env.is_none() {
            probe.code = read_code_facts(&buf);
        }
    } else if want_sha1 {
        // 只为哈希：分块流式读，不把整包塞进内存
        let mut h = Sha1::new();
        let mut buf = [0u8; 64 * 1024];
        loop {
            match entry.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => h.update(&buf[..n]),
                Err(_) => return None,
            }
        }
        probe.sha1 = Some(hex(&h.finalize()));
    } else {
        // 大 jar 且只要能自证元数据：不整包进内存，跳过（交给平台层）
        return None;
    }
    Some(probe)
}

fn sha1_hex(bytes: &[u8]) -> String {
    let mut h = Sha1::new();
    h.update(bytes);
    hex(&h.finalize())
}

/// 用户「从本地添加」的单个 jar：整文件当 zip 解，取证口径与包内条目完全一致
/// （自证端 + 自报身份 + 字节 sha1 + 字节码提示）。超大 jar 只算哈希，不整包解元数据。
pub fn probe_local_jar(path: &Path) -> JarProbe {
    let mut probe = JarProbe::default();
    let Ok(bytes) = std::fs::read(path) else {
        return probe;
    };
    probe.sha1 = Some(sha1_hex(&bytes));
    if bytes.len() as u64 <= JAR_MAX_UNCOMPRESSED {
        read_meta(&bytes, &mut probe);
        // 同包内条目：加载器元数据已自证端就不必再解 class
        if probe.env.is_none() {
            probe.code = read_code_facts(&bytes);
        }
    }
    probe
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// 从 jar 内的元数据取身份与自证端（多取几个文件：Fabric 系一个、Forge 系一个）
fn read_meta(jar: &[u8], probe: &mut JarProbe) {
    let Ok(mut inner) = zip::ZipArchive::new(Cursor::new(jar)) else {
        return;
    };
    for name in JAR_META {
        let Ok(mut meta) = inner.by_name(name) else {
            continue;
        };
        let mut raw = Vec::new();
        if meta.read_to_end(&mut raw).is_err() {
            continue;
        }
        let Ok(text) = String::from_utf8(raw) else {
            continue;
        };
        if name.ends_with(".json") {
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
                continue;
            };
            set_if_empty(&mut probe.mod_id, &json_str(&v, "id"));
            set_if_empty(&mut probe.title, &json_str(&v, "name"));
            // fabric: 顶层 environment = "*" | "client" | "server"；quilt 同名字段一并读
            let env = json_str(&v, "environment");
            let sides = match env.trim().to_lowercase().as_str() {
                "client" => Some((SideFlag::Required, SideFlag::Unsupported)),
                "server" => Some((SideFlag::Unsupported, SideFlag::Required)),
                "*" => Some((SideFlag::Optional, SideFlag::Optional)),
                // 作者压根没写 environment：退到结构证据（entrypoints 段）
                _ => entrypoint_sides(&v),
            };
            if let Some((client, server)) = sides {
                if probe.env.is_none() {
                    probe.env = Some(Evidence {
                        client: Some(client),
                        server: Some(server),
                        source: EnvSource::JarMetadata,
                    });
                }
            }
        } else {
            // Forge / NeoForge 的 mods.toml：无端字段，只有身份可取
            set_if_empty(&mut probe.mod_id, &toml_field(&text, "modId"));
            set_if_empty(&mut probe.title, &toml_field(&text, "displayName"));
        }
    }
}

/// Fabric/Quilt 的 `entrypoints` 段 → 端证据，只在作者没写 `environment` 时进场。
/// 有 `main`（common 入口）就说明服务端装了确实有代码在跑，这种行绝不剔；
/// 只有 `client` 入口、没有 common 入口，才是「代码只在客户端物理端」的结构事实。
/// 两条路都拿不到时返回 None（交回上层，宁可留不剔）。
fn entrypoint_sides(v: &serde_json::Value) -> Option<(SideFlag, SideFlag)> {
    let ep = v.get("entrypoints")?.as_object()?;
    let registered = |k: &str| {
        ep.get(k).is_some_and(|e| {
            e.as_array().map(|a| !a.is_empty()).unwrap_or(true)
        })
    };
    if registered("main") {
        return Some((SideFlag::Optional, SideFlag::Optional));
    }
    registered("client").then_some((SideFlag::Required, SideFlag::Unsupported))
}

fn set_if_empty(slot: &mut Option<String>, value: &str) {
    if slot.as_deref().is_none_or(|s| s.is_empty()) {
        let v = value.trim();
        if !v.is_empty() {
            *slot = Some(v.to_string());
        }
    }
}

fn json_str(v: &serde_json::Value, key: &str) -> String {
    v.get(key).and_then(|s| s.as_str()).unwrap_or_default().to_string()
}

/// mods.toml 里取第一个 `key = "value"`（无 TOML 依赖：只取一个字符串字段，够用）
fn toml_field(text: &str, key: &str) -> String {
    for line in text.lines() {
        let line = line.trim_start();
        let Some(rest) = line.strip_prefix(key) else {
            continue;
        };
        let Some((_, value)) = rest.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_start_matches(['"', '\'']);
        let value = value.split(['"', '\'']).next().unwrap_or_default();
        if !value.is_empty() {
            return value.to_string();
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::env::fixtures::{class_bytes, zip_bytes};

    #[test]
    fn toml_field_reads_first_key_value() {
        let toml = r#"
modId = "create"
[[mods]]
modId="geckolib3"
displayName = 'GeckoLib'
"#;
        assert_eq!(toml_field(toml, "modId"), "create");
        assert_eq!(toml_field(toml, "displayName"), "GeckoLib");
        assert_eq!(toml_field(toml, "logoFile"), "");
    }

    #[test]
    fn entrypoints_answer_when_author_declared_no_environment() {
        let parse = |s: &str| serde_json::from_str::<serde_json::Value>(s).unwrap();
        // 只有 client 入口：代码全在客户端物理端 → 剔除候选
        assert_eq!(
            entrypoint_sides(&parse(r#"{"entrypoints":{"client":["a;b"]}}"#)),
            Some((SideFlag::Required, SideFlag::Unsupported))
        );
        // 有 main（common）入口：服务端装了确实有代码在跑，绝不剔
        assert_eq!(
            entrypoint_sides(&parse(r#"{"entrypoints":{"client":["a"],"main":["b"]}}"#)),
            Some((SideFlag::Optional, SideFlag::Optional))
        );
        // 注册段存在但空数组不算注册；两段都没有 → 无证据（宁可留不剔）
        assert_eq!(entrypoint_sides(&parse(r#"{"entrypoints":{"client":[]}}"#)), None);
        assert_eq!(entrypoint_sides(&parse(r#"{"id":"x"}"#)), None);
    }

    #[test]
    fn probe_local_jar_reads_a_standalone_file_the_same_way() {
        // 用户「从本地添加」的 jar 不在任何包里：同一个探测口径要能直接吃单个文件
        let jar = zip_bytes(&[(
            "fabric.mod.json",
            br#"{"id":"ksyxis","name":"Ksyxis","environment":"server"}"#,
        )]);
        let dir = std::env::temp_dir().join(format!(
            "sideshift-env-local-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ksyxis-1.2.jar");
        std::fs::write(&path, &jar).unwrap();

        let p = probe_local_jar(&path);
        assert_eq!(p.sha1.as_deref(), Some(sha1_hex(&jar).as_str()));
        assert_eq!(p.mod_id.as_deref(), Some("ksyxis"));
        let ev = p.env.expect("fabric environment=server 应自证");
        assert_eq!(ev.client, Some(SideFlag::Unsupported));
        assert_eq!(ev.server, Some(SideFlag::Required));
        assert_eq!(ev.source, EnvSource::JarMetadata);

        // 读不到的文件不该把命令层炸掉：静默出空探测，前端落「需人工确认」
        assert_eq!(
            probe_local_jar(&dir.join("missing.jar")).sha1,
            None
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn probe_jars_reads_self_declared_env_sha1_and_identity() {
        // 故意塞一个带服务端标记的 class：fabric.mod.json 已自证 environment，
        // 字节码扫描就该被完全跳过（下面 `p.code == default` 断的是这条闸门）
        let jar = zip_bytes(&[
            (
                "fabric.mod.json",
                br#"{"id":"skinlayers3d","name":"3D Skin Layers","environment":"client"}"#,
            ),
            (
                "com/example/Setup.class",
                class_bytes(&["com/example/Setup", "DeferredRegister"]).as_slice(),
            ),
        ]);
        let pack = zip_bytes(&[("mods/万用皮肤补丁.jar", &jar)]);
        let dir = std::env::temp_dir().join(format!("sideshift-env-probe-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pack.zip");
        std::fs::write(&path, &pack).unwrap();

        let probes = probe_jars(
            &path,
            &[ProbeReq {
                path: "mods/万用皮肤补丁.jar".to_string(),
                want_sha1: true,
                want_code: true,
            }],
        );
        let p = probes
            .get("mods/万用皮肤补丁.jar")
            .expect("条目应按包内路径命中");
        assert_eq!(p.sha1.as_deref(), Some(sha1_hex(&jar).as_str()));
        assert_eq!(p.mod_id.as_deref(), Some("skinlayers3d"));
        assert_eq!(p.title.as_deref(), Some("3D Skin Layers"));
        let ev = p.env.expect("fabric environment=client 应自证");
        assert_eq!(ev.client, Some(SideFlag::Required));
        assert_eq!(ev.server, Some(SideFlag::Unsupported));
        assert_eq!(ev.source, EnvSource::JarMetadata);
        // fabric.mod.json 已自证端 → 不必再解 class，字节码层留空
        assert_eq!(p.code, CodeFacts::default());
        // 只要元数据的请求（mrpack 已给哈希）不该产出 sha1——那是白读整包
        let light = probe_jars(
            &path,
            &[ProbeReq {
                path: "mods/万用皮肤补丁.jar".to_string(),
                want_sha1: false,
                want_code: false,
            }],
        );
        assert_eq!(
            light.get("mods/万用皮肤补丁.jar").and_then(|p| p.sha1.as_ref()),
            None
        );
        assert_eq!(
            light
                .get("mods/万用皮肤补丁.jar")
                .and_then(|p| p.mod_id.as_deref()),
            Some("skinlayers3d")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
