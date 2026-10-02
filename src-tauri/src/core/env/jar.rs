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
    /// jar 原始字节的 CF murmur2 指纹（`cf_fingerprint`）——Modrinth 哈希反查落空的
    /// CF 独占模组，凭它向 CF 按文件反查（`POST /v1/fingerprints`）。只在整包哈希
    /// 已在算的那批（`want_sha1`）与单个本地 jar 上算：mrpack 行的哈希由清单自带，
    /// 多算一遍指纹等于把最便宜的离线路变贵
    pub cf_fingerprint: Option<u32>,
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

/// 一个待探测条目：包内条目下标 + 包内路径 + 两条「要不要跑这条腿」的闸门
type JarTarget = (usize, String, bool, bool);

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
    let Ok(mut archive) = zip::ZipArchive::new(file) else {
        return out;
    };
    let targets: Vec<JarTarget> = archive
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
    // 重量取条目自己的解压后字节数（`by_index` 只定位到本条、不解压），零额外读字节成本
    let sized = targets
        .into_iter()
        .map(|t| (archive.by_index(t.0).map(|e| e.size()).unwrap_or(0), t))
        .collect();

    let mut results: Vec<Vec<(String, JarProbe)>> = Vec::new();
    std::thread::scope(|s| {
        let handles: Vec<_> = pack_bins(sized, JAR_THREADS)
            .into_iter()
            .map(|bin| {
                let pack = pack.to_path_buf();
                s.spawn(move || jar_chunk(&pack, &bin))
            })
            .collect();
        for h in handles {
            if let Ok(r) = h.join() {
                results.push(r);
            }
        }
    });
    for bin in results {
        for (path, probe) in bin {
            out.insert(path, probe);
        }
    }
    out
}

/// 把条目按解压后字节数装进 `threads` 个箱：字节数**降序**依次进当前最轻的一箱（LPT 贪心）。
///
/// 原来按清单顺序连续切片 ⇒ 谁摊上 Create 那种大块头谁就是整条关键路径，其余线程早早收工干等
/// （实测 76 行四段合计 6042ms、墙钟却 3559ms ⇒ 四条线程平均只跑出 1.7 条）。装箱后墙钟≈合计 ÷ 线程数
fn pack_bins(sized: Vec<(u64, JarTarget)>, threads: usize) -> Vec<Vec<JarTarget>> {
    let mut sized = sized;
    sized.sort_unstable_by(|a, b| b.0.cmp(&a.0));
    // `.max(1)`：线程数被拧成 0 时这里会开出零个箱，于是每条都进不了箱——**整层静默不扫**，
    // 读起来像「缓存生效、秒回」，比崩掉难查得多
    let mut bins: Vec<(u64, Vec<JarTarget>)> =
        (0..threads.max(1).min(sized.len()))
            .map(|_| (0u64, Vec::new()))
            .collect();
    for (size, target) in sized {
        // 箱数 ≥1（上面那道 `max(1)`），线性找最轻：条目 ≤400、箱个位数
        let lightest = bins.iter_mut().min_by_key(|(total, _)| *total);
        if let Some((total, bin)) = lightest {
            *total += size;
            bin.push(target);
        }
    }
    bins.into_iter().map(|(_, bin)| bin).collect()
}

/// 一个箱：重开包句柄，逐个 jar 探测（箱内串行，箱间并行）
fn jar_chunk(pack: &Path, chunk: &[JarTarget]) -> Vec<(String, JarProbe)> {
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
        let read_ok = entry.read_to_end(&mut buf).is_ok();
        if !read_ok {
            return None;
        }
        if want_sha1 {
            probe.sha1 = Some(sha1_hex(&buf));
            // 指纹与哈希吃同一份内存里的字节：CF 指纹腿的入场券，别处不补算
            probe.cf_fingerprint = Some(cf_fingerprint(&buf));
        }
        read_meta(&buf, &mut probe);
        // 加载器元数据已自证端就别再解 class 了：Fabric/Quilt 的 `environment` 更强也更便宜
        if want_code && probe.env.is_none() {
            probe.code = read_code_facts(&buf);
        }
    } else if want_sha1 {
        // 只为哈希：分块流式读，不把整包塞进内存。指纹不在流式路径上算——
        // 它的受众是「哈希答不上的 CF 独占模组」，超大 jar 本就罕见，别为它养第二条读带
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
/// （自证端 + 自报身份 + 字节 sha1 + CF 指纹 + 字节码提示）。超大 jar 只算哈希，不整包解元数据。
pub fn probe_local_jar(path: &Path) -> JarProbe {
    let mut probe = JarProbe::default();
    let Ok(bytes) = std::fs::read(path) else {
        return probe;
    };
    probe.sha1 = Some(sha1_hex(&bytes));
    // 单个文件一条：指纹免费，恒算（在线添加版本列表那档没有字节，走 resolve_added_build）
    probe.cf_fingerprint = Some(cf_fingerprint(&bytes));
    if bytes.len() as u64 <= JAR_MAX_UNCOMPRESSED {
        read_meta(&bytes, &mut probe);
        // 同包内条目：加载器元数据已自证端就不必再解 class
        if probe.env.is_none() {
            probe.code = read_code_facts(&bytes);
        }
    }
    probe
}

/// CurseForge 的文件指纹：MurmurHash2（32 位、seed=1），先剔除四种空白字节（\t \n \r 空格）。
/// **2026-10 用真值校准过**：官方 API 报 `fileFingerprint: 3020746965` 的那份文件
/// （modId 369096 / fileId 4136487，edge.forgecdn.net 直链可下），按此实现逐位一致；
/// 不剔除空白则得 1550895763，对不上。这是 `POST /v1/fingerprints` 的入场券
/// （见 `curseforge_fingerprints`），校验和 sha1 各吃一遍内存里的同一份字节
pub fn cf_fingerprint(bytes: &[u8]) -> u32 {
    const M: u32 = 0x5bd1e995;
    const R: u32 = 24;
    const SEED: u32 = 1;
    let is_ws = |b: u8| matches!(b, b'\t' | b'\n' | b'\r' | b' ');
    // seed 要先与总长异或才开始混（murmur2 的规范形状），所以先数一遍有效字节；
    // 两遍线性扫不分配内存，比把剔除后的字节囤一份省得多
    let len = bytes.iter().filter(|b| !is_ws(**b)).count() as u32;
    let mut h: u32 = SEED ^ len;
    let mut acc = [0u8; 4];
    let mut fill = 0usize;
    for &b in bytes {
        if is_ws(b) {
            continue;
        }
        acc[fill] = b;
        fill += 1;
        if fill == 4 {
            let mut k = u32::from_le_bytes(acc);
            k = k.wrapping_mul(M);
            k ^= k >> R;
            k = k.wrapping_mul(M);
            h = h.wrapping_mul(M) ^ k;
            fill = 0;
        }
    }
    // 尾块按规范 fall-through：3/2/1 各档依次叠，只在第 1 档后乘一次
    if fill >= 3 {
        h ^= (acc[2] as u32) << 16;
    }
    if fill >= 2 {
        h ^= (acc[1] as u32) << 8;
    }
    if fill >= 1 {
        h ^= acc[0] as u32;
        h = h.wrapping_mul(M);
    }
    h ^= h >> 13;
    h = h.wrapping_mul(M);
    h ^= h >> 15;
    h
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

    /// 小向量锁 seed、尾块与空白剔除三处边角；真文件校准（官方 fileFingerprint 逐位一致）
    /// 见 `cf_fingerprint` 的注释。空白字节（空格/tab/LF/CR）先剔除再混：
    /// `hello world` 与 `helloworld` 同值、`a b\nc\td\re` 与 `abcde` 同值
    #[test]
    fn cf_fingerprint_strips_whitespace_and_matches_reference() {
        assert_eq!(cf_fingerprint(b"hello world"), 2824650221);
        assert_eq!(cf_fingerprint(b"helloworld"), 2824650221);
        assert_eq!(cf_fingerprint(b"a b\nc\td\re"), 3469237630);
        assert_eq!(cf_fingerprint(b"abcde"), 3469237630);
        assert_eq!(cf_fingerprint(b""), 1540447798);
    }

    /// 装箱按字节数配平，不再按清单顺序连续切：同一批「大在前、小在后」的条目，连续切三箱
    /// 最重那箱 21，装箱后压到 13（合计 36 ÷ 3 = 12 是理想下界）
    #[test]
    fn bins_balance_by_size_not_by_list_order() {
        // 借条目下标那一格存字节数，事后按它把每箱重量加回来
        let sized = [8u64, 7, 6, 5, 4, 3, 2, 1]
            .into_iter()
            .map(|size| (size, (size as usize, String::new(), false, false)))
            .collect();
        let bins = pack_bins(sized, 3);
        assert_eq!(bins.iter().map(|b| b.len()).sum::<usize>(), 8);
        assert!(bins.iter().all(|b| !b.is_empty()), "八条三箱不该有空箱");
        let heaviest = bins
            .iter()
            .map(|b| b.iter().map(|t| t.0 as u64).sum::<u64>())
            .max()
            .unwrap();
        assert_eq!(heaviest, 13);
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
