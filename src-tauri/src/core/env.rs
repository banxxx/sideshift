//! 端判定证据采集：判定一个模组「服务端要不要」的取证层，不含裁决规则（裁决在 detector）。
//!
//! 证据阶梯（可信度由高到低，detector 依 `rank` 取第一层命中的）：
//! 1. 包内 jar 的 `fabric.mod.json:environment`（没写则看 `entrypoints` 段）——模组作者自证，
//!    且是加载器运行时真在执行的声明（本模块离线扫描）
//! 2. Modrinth 按文件 sha1 反查构建的 `environment`——平台侧、精确到这一个文件
//!    （裸 zip 的 index 没给哈希 → 本模块扫描时自行计算 jar 字节 sha1）
//! 3. Modrinth 项目级 `client_side/server_side`——按文件名/包内自报 id 推出的 slug，
//!    名字对得上才采信；再兜不住按模组显示名走 `/v2/search`（第 3 层内，同样标平台项目）
//! 4. `mrpack files[].env`——打包者抄的第二手声明，整表无区分度时整层作废（见 detector）
//! 5. detector 的模组名关键字表——纯猜，兜底；jar 里有服务端注册事实时不许它开口
//!
//! 第 3、4 层是网络查询，结果落 `cache_dir/env-index.json`：同一个模组第二次见到即离线可答。
//! Forge / NeoForge 的 `META-INF/mods.toml` 没有自证端字段（实测），所以第 2 层只覆盖 Fabric / Quilt；
//! 那里补一道**字节码结构提示**（`read_code_facts`）：只用来按住名称关键字层的误删与给一句提示，
//! 不参与剔除——实测「引用了哪些 MC 类」区分不了两端，能区分的只有加载器的注册 API。
//! 但 jar 内的 `id` / `displayName` 仍然有价值：国内整合包常把 jar 文件名整体改成中文，
//! 此时文件名推不出任何线索，只有包内自报身份能把行对上 Modrinth 项目。

use std::collections::HashMap;
use std::fs::File;
use std::io::{Cursor, Read};
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};

use crate::core::downloader::{Downloader, ModrinthEnv};
use crate::core::parser::PackFile;
use crate::models::{EnvSource, SideFlag};

/// 一条两侧支持度证据及其出处
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Evidence {
    pub client: Option<SideFlag>,
    pub server: Option<SideFlag>,
    pub source: EnvSource,
}

/// 包内条目路径 → 证据（第 2~4 层的汇总；第 1 层由 parser 挂在 PackFile 上）
pub type EvidenceMap = HashMap<String, Evidence>;

/// 来源可信度排序（越小越可信）。
/// **mrpack 的 `files[].env` 排在 jar 自证与平台反查之后**：它是打包者抄下来的第二手声明，
/// 第三方工具还普遍把整表刷成 required/required（一个支持度都没说）。jar 内的
/// `environment` 是加载器运行时强制执行的，平台侧声明则由模组作者在自己项目上维护。
pub fn rank(s: EnvSource) -> u8 {
    match s {
        EnvSource::JarMetadata => 0,
        EnvSource::ModrinthHash => 1,
        EnvSource::ModrinthProject => 2,
        EnvSource::Mrpack => 3,
        EnvSource::NameHeuristic => 4,
        EnvSource::Unknown => 5,
    }
}

/// 写入证据：仅在来源更可信（或同级＝更新）时覆盖
pub fn put(map: &mut EvidenceMap, path: &str, ev: Evidence) {
    if map
        .get(path)
        .is_some_and(|cur| rank(cur.source) < rank(ev.source))
    {
        return;
    }
    map.insert(path.to_string(), ev);
}

/// Modrinth `environment` 枚举 → 两侧支持度（project 与 version 同枚举，实测六值）
fn sides_from_environment(env: &str) -> Option<(SideFlag, SideFlag)> {
    use SideFlag::{Optional, Required, Unsupported};
    let norm = env.trim().to_lowercase().replace('-', "_");
    Some(match norm.as_str() {
        "client_only" => (Required, Unsupported),
        "client_only_server_optional" => (Required, Optional),
        "server_only" => (Unsupported, Required),
        "server_only_client_optional" => (Optional, Required),
        "client_and_server" => (Required, Required),
        "client_or_server" | "client_or_server_prefers_both" => (Optional, Optional),
        _ => return None,
    })
}

fn side_flag(v: &str) -> Option<SideFlag> {
    Some(match v.trim().to_lowercase().as_str() {
        "required" => SideFlag::Required,
        "optional" => SideFlag::Optional,
        "unsupported" => SideFlag::Unsupported,
        _ => return None,
    })
}

/// Modrinth 端信息 → 证据：优先精确的 client_side/server_side，回落 environment 枚举
fn evidence_from_modrinth(m: &ModrinthEnv, source: EnvSource) -> Option<Evidence> {
    if let (Some(c), Some(s)) = (
        m.client_side.as_deref().and_then(side_flag),
        m.server_side.as_deref().and_then(side_flag),
    ) {
        return Some(Evidence {
            client: Some(c),
            server: Some(s),
            source,
        });
    }
    m.environment
        .as_deref()
        .and_then(sides_from_environment)
        .map(|(c, s)| Evidence {
            client: Some(c),
            server: Some(s),
            source,
        })
}

/* ---------------- 第 2 层：包内 jar 元数据（离线） ---------------- */

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

/* ---------------- 字节码结构提示（加载器元数据没自证端时的最后一道保险） ---------------- */

/// jar 字节的结构事实。用途被刻意限制在「少删」这一侧——13 个真实 Modrinth 包实测：
/// 纯客户端模组照样大量引用 `net/minecraft/world/`（要读实体和方块才渲染得出来），
/// 两端包也照样引用 `net/minecraft/client/`。「引用了哪些 MC 类」根本区分不了两端，
/// 唯一分得开的是**加载器自己的注册 API**（这部分不参与混淆，Forge 侧也一样是明文）：
/// 3 个 client_only 模组（新旧两个版本各测一遍）服务端标记 0 命中，
/// AppleSkin / JEI / Create 全部 ≥2 命中。所以这里只允许两种结论：
/// 「确有服务端注册 → 别靠猜名字删它」与「形状像纯客户端 → 只提示、不删」。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CodeFacts {
    /// 有只有服务端才会跑的注册（common 生命周期 / 注册表 / 网络 payload / 服务端类）
    pub server_code: bool,
    /// 只见客户端生命周期注册、不见任何服务端注册：形状像纯客户端（不据此剔除）
    pub client_only_shape: bool,
}

/// 包内条目路径 → 字节码结构事实（与 `EvidenceMap` 平行的一条独立通道：
/// 它不是证据层级的一员，只在 detector 里当「名称关键字层准不准开口」的闸门）
pub type CodeMap = HashMap<String, CodeFacts>;

/// 服务端注册的词汇表。刻意不收 `net/minecraft/world/`、`net/minecraftforge/common/`
/// 这类宽口径——纯客户端模组也在用（改包后实测会把 Entity Culling 判成服务端必需）
const SERVER_TOKENS: &[&str] = &[
    "FMLCommonSetupEvent",
    "CommonSetupEvent",
    "RegisterEvent",
    "DeferredRegister",
    "RegisterPayloadHandlersEvent",
    "InterModComms",
    "net/minecraft/server/",
    "ServerAboutToStartEvent",
    "ServerStartingEvent",
    "RegisterCapabilitiesEvent",
    "AddReloadListenerEvent",
    "EntityAttributeCreationEvent",
    "FMLPreInitializationEvent",
    "FMLInitializationEvent",
    "ServerLifecycleEvents",
];

/// 客户端注册的词汇表（只为「形状」提示服务，不参与任何剔除决定）
const CLIENT_TOKENS: &[&str] = &[
    "FMLClientSetupEvent",
    "ClientSetupEvent",
    "RegisterGuiLayersEvent",
    "ClientTickEvent",
    "ClientPlayerNetworkEvent",
    "RegisterParticleProvidersEvent",
    "RenderTickEvent",
];

/// 单个 jar 最多解多少个 class：命中服务端标记即提前收工，所以这条只兜住
/// 「纯客户端的大 jar」那种最坏情况（解 class 是离线层最贵的一步）
const CODE_MAX_CLASSES: usize = 4000;

/// 扫 jar 内所有 class 的常量池，得出结构事实。读不动的条目静默跳过（交回上层）
pub fn read_code_facts(jar: &[u8]) -> CodeFacts {
    let mut facts = CodeFacts::default();
    let Ok(mut archive) = zip::ZipArchive::new(Cursor::new(jar)) else {
        return facts;
    };
    let mut seen = 0usize;
    let mut client_hit = false;
    for i in 0..archive.len() {
        let Ok(name) = archive.by_index(i).map(|f| f.name().to_string()) else {
            continue;
        };
        if !name.ends_with(".class") || name.starts_with("META-INF/versions/") {
            continue;
        }
        if seen >= CODE_MAX_CLASSES {
            break;
        }
        seen += 1;
        let mut buf = Vec::new();
        let Ok(mut entry) = archive.by_index(i) else {
            continue;
        };
        if entry.read_to_end(&mut buf).is_err() {
            continue;
        }
        // 找到服务端事实就收工：它的结论（不许删）已经是这轮能给出的最强事实
        if utf8_entries(&buf, &mut |s| {
            if !client_hit && CLIENT_TOKENS.iter().any(|t| s.contains(t)) {
                client_hit = true;
            }
            SERVER_TOKENS.iter().any(|t| s.contains(t))
        }) {
            facts.server_code = true;
            return facts;
        }
    }
    facts.client_only_shape = client_hit;
    facts
}

/// 逐个把 class 常量池里的 Utf8 串交给 `visit`；`visit` 返回 true 时提前收工。
/// 只走常量池、不进字段/方法/属性区：那里曾有属性长度错位的老坑（Python 原型踩过），
/// 而这些标记全都在常量池里，不必冒结构错位的险。实测 7000+ 个 class 零解析失败
fn utf8_entries(b: &[u8], visit: &mut impl FnMut(&str) -> bool) -> bool {
    /// JVMS 4.4：tag → 该条目在 tag 之后占几字节（Long/Double 占两槽，见外层处理）
    const fn size(tag: u8) -> Option<usize> {
        Some(match tag {
            1 => return None, // 变长，外层单独处理
            3 | 4 => 4,
            5 | 6 => 8,
            7 | 8 | 16 | 19 | 20 | 21 => 2,
            9 | 10 | 11 | 12 | 17 | 18 => 4,
            15 => 3,
            _ => return None,
        })
    }
    if b.len() < 10 || b[..4] != [0xCA, 0xFE, 0xBA, 0xBE] {
        return false;
    }
    let count = u16::from_be_bytes([b[8], b[9]]) as usize;
    let mut p = 10usize;
    let mut i = 1usize;
    while i < count {
        let Some(tag) = b.get(p) else { return false };
        p += 1;
        if *tag == 1 {
            let Some(&[l0, l1]) = b.get(p..p + 2) else {
                return false;
            };
            let len = u16::from_be_bytes([l0, l1]) as usize;
            p += 2;
            let Some(text) = b.get(p..p + len).and_then(|s| std::str::from_utf8(s).ok()) else {
                return false;
            };
            p += len;
            if visit(text) {
                return true;
            }
        } else {
            let Some(n) = size(*tag) else { return false };
            p += n;
            // Long / Double 占两个常量池槽位
            if *tag == 5 || *tag == 6 {
                i += 1;
            }
        }
        i += 1;
    }
    false
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

/* ---------------- 第 3/4 层：Modrinth 反查（在线 + 本地索引） ---------------- */

const INDEX_FILE: &str = "env-index.json";

fn sha1_key(h: &str) -> String {
    format!("sha1:{}", h.to_lowercase())
}
fn project_key(p: &str) -> String {
    format!("proj:{}", p.to_lowercase())
}

/// `cache_dir/env-index.json`：sha1 / 项目 → 端证据。联网层查到的结果写这里，
/// 下次同一模组（哪怕在另一个包里）离线即答。
#[derive(Default, Deserialize)]
pub struct EnvIndex {
    #[serde(default)]
    map: HashMap<String, Evidence>,
}

impl EnvIndex {
    pub fn load(cache_dir: &Path) -> Self {
        std::fs::read_to_string(cache_dir.join(INDEX_FILE))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn save(&self, cache_dir: &Path) {
        if let Ok(json) = serde_json::to_string(&self.map) {
            let _ = std::fs::create_dir_all(cache_dir);
            let _ = std::fs::write(cache_dir.join(INDEX_FILE), json);
        }
    }

    fn get(&self, key: &str) -> Option<Evidence> {
        self.map.get(key).copied()
    }

    /// 本地索引是否已答过这一份字节：答过就不必为它解 class（命令层的省钱闸门）
    pub fn has_sha1(&self, sha1: &str) -> bool {
        self.map.contains_key(&sha1_key(sha1))
    }

    fn record(&mut self, key: String, ev: Evidence) {
        self.map.insert(key, ev);
    }
}

/// 一个待反查的模组行
#[derive(Clone)]
pub struct Target {
    /// 包内条目路径（EvidenceMap 的键）
    pub path: String,
    pub sha1: Option<String>,
    /// 权威项目引用：Modrinth 下载 URL 里的 project_id，查到即采信
    pub project_id: Option<String>,
    /// slug 候选（模组自报 id 在前，文件名切出的 id 在后）；返回项目名字对得上才采信
    pub slugs: Vec<String>,
    /// 可读模组名（按名搜索兜底；优先取 jar 内作者写的显示名）
    pub title: Option<String>,
}

/// 把 jar 自报的身份与哈希补进反查目标：index 没给 sha1 的行（裸 zip、手动塞进 mods 的 jar）
/// 靠扫描算出的哈希走第 3 层，靠包内 id/显示名走第 4 层——中文改名只剩这条路可走。
pub fn apply_probes(probes: &HashMap<String, JarProbe>, targets: &mut [Target]) {
    for t in targets.iter_mut() {
        let Some(p) = probes.get(&t.path) else {
            continue;
        };
        if t.sha1.is_none() {
            t.sha1 = p.sha1.clone();
        }
        if let Some(id) = p.mod_id.as_deref().map(|s| s.to_lowercase()) {
            if is_slug(&id) && t.slugs.first().map(|s| s.as_str()) != Some(id.as_str()) {
                t.slugs.insert(0, id);
            }
        }
        if t.title.is_none() {
            t.title = p.title.clone();
        }
    }
}

/// 用本地索引填空缺（不发请求）；返回仍需联网的 target 下标
pub fn apply_index(index: &EnvIndex, targets: &[Target], out: &mut EvidenceMap) -> Vec<usize> {
    let mut pending = Vec::new();
    for (i, t) in targets.iter().enumerate() {
        // jar 自证已答上的行不进反查队列：那是最高可信层，再查一遍只会更差
        if out.contains_key(&t.path) {
            continue;
        }
        let by_proj = t
            .project_id
            .as_deref()
            .into_iter()
            .chain(t.slugs.iter().map(|s| s.as_str()))
            .chain(t.title.as_deref())
            .find_map(|p| index.get(&project_key(p)));
        let hit = t
            .sha1
            .as_deref()
            .and_then(|h| index.get(&sha1_key(h)))
            .or(by_proj);
        match hit {
            Some(ev) => put(out, &t.path, ev),
            None => pending.push(i),
        }
    }
    pending
}

/// 逐行项目/搜索反查的请求上限：超大包剩下的行留未判定（离线结论照常有效），
/// 免得一次转换打满 Modrinth 限流（300 req/min）影响后续下载
const MAX_LOOKUP_REQUESTS: usize = 150;

/// 在线反查：sha1 批量优先，未命中的按项目/模组名逐个查；结果同时写回 out 与索引。
/// 返回 false = 有请求失败或超出请求上限（前端提示「联网反查未全部完成」）。
pub async fn resolve_online(
    dl: &Downloader,
    index: &mut EnvIndex,
    cache_dir: &Path,
    targets: &[Target],
    pending: &[usize],
    out: &mut EvidenceMap,
) -> bool {
    let mut ok = true;

    // 第 3 层：按 sha1 批量反查构建（接口上限 1000 个哈希，这里按 200 分批）
    let hashes: Vec<String> = pending
        .iter()
        .filter_map(|i| targets[*i].sha1.clone())
        .map(|h| h.to_lowercase())
        .collect();
    let mut hits: HashMap<String, ModrinthEnv> = HashMap::new();
    for chunk in hashes.chunks(200) {
        match dl.version_env_by_sha1(chunk).await {
            Ok(m) => hits.extend(m),
            Err(_) => ok = false,
        }
    }
    let mut unresolved = Vec::new();
    for i in pending {
        let t = &targets[*i];
        let by_hash = t
            .sha1
            .as_deref()
            .and_then(|h| hits.get(&h.to_lowercase()))
            .and_then(|m| evidence_from_modrinth(m, EnvSource::ModrinthHash));
        match by_hash {
            Some(ev) => {
                put(out, &t.path, ev);
                if let Some(h) = &t.sha1 {
                    index.record(sha1_key(h), ev);
                }
                // 同一份结论也挂在项目键下：项目级 env 通常跨版本稳定
                for p in t.project_id.iter().chain(t.slugs.iter()) {
                    index.record(project_key(p), ev);
                }
            }
            None => unresolved.push(*i),
        }
    }

    // 第 4 层：项目级 client_side / server_side（哈希不在 Modrinth 上的条目、裸 zip 条目）
    let mut requests = 0usize;
    let mut by_name = Vec::new();
    for i in unresolved {
        let t = &targets[i];
        let mut got = None;
        // 权威 = URL 里带出的 project_id（查到即采信）；slug 候选由 id/文件名猜出，
        // 返回项目的 slug/title 与候选对得上才采信（猜错项目比漏判更糟：会误删别的模组）
        for (key, authoritative) in t
            .project_id
            .as_deref()
            .map(|k| (k, true))
            .into_iter()
            .chain(t.slugs.iter().map(|k| (k.as_str(), false)))
        {
            if requests >= MAX_LOOKUP_REQUESTS {
                ok = false;
                break;
            }
            requests += 1;
            match dl.project_env(key).await {
                Ok(Some(m)) => {
                    if !authoritative && !slug_confident(key, &m) {
                        continue;
                    }
                    if let Some(ev) = evidence_from_modrinth(&m, EnvSource::ModrinthProject) {
                        got = Some((resolved_key(&m, key), ev));
                        break;
                    }
                }
                Ok(None) => {}
                Err(_) => ok = false,
            }
        }
        match got {
            Some((keys, ev)) => {
                put(out, &t.path, ev);
                for k in keys {
                    index.record(project_key(&k), ev);
                }
            }
            // 项目都猜不上：留着按显示名搜（中文名改过的包只剩这一层）
            None => by_name.push(i),
        }
    }

    // 第 4 层补充：按模组显示名走 /v2/search（命中项自带 client_side/server_side/environment）
    for i in by_name {
        let t = &targets[i];
        let Some(query) = t.title.as_deref().or(t.slugs.first().map(|s| s.as_str())) else {
            continue;
        };
        if requests >= MAX_LOOKUP_REQUESTS {
            ok = false;
            break;
        }
        requests += 1;
        match dl.search_env(query).await {
            Ok(list) => {
                let Some(m) = pick_search_hit(query, t, &list) else {
                    continue;
                };
                if let Some(ev) = evidence_from_modrinth(m, EnvSource::ModrinthProject) {
                    put(out, &t.path, ev);
                    for k in resolved_key(m, query) {
                        index.record(project_key(&k), ev);
                    }
                }
            }
            Err(_) => ok = false,
        }
    }

    index.save(cache_dir);
    ok
}

/// 结论挂在哪些键下：返回项目的 slug/id + 本次查询用的键，下次离线即答
fn resolved_key(m: &ModrinthEnv, queried: &str) -> Vec<String> {
    let mut keys: Vec<String> = [m.slug.as_deref(), Some(queried)]
        .into_iter()
        .flatten()
        .map(|k| k.to_lowercase())
        .collect();
    keys.dedup();
    keys
}

/// 搜索结果采信口径：只认「归一化后完全等于查询名/候选 slug」的那一条，不做模糊匹配
fn pick_search_hit<'a>(query: &str, t: &Target, hits: &'a [ModrinthEnv]) -> Option<&'a ModrinthEnv> {
    let want: Vec<String> = [Some(query), t.title.as_deref()]
        .into_iter()
        .flatten()
        .chain(t.slugs.iter().map(|s| s.as_str()))
        .map(norm_key)
        .filter(|s| !s.is_empty())
        .collect();
    hits.iter().find(|m| {
        [m.slug.as_deref(), m.title.as_deref()]
            .into_iter()
            .flatten()
            .any(|s| want.iter().any(|w| w == &norm_key(s)))
    })
}

/// 只留字母数字并小写：比较模组名的两种写法
fn norm_key(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// slug 候选 ↔ 返回项目是否同一个：只比字母数字（"fabric-api" ↔ "FabricAPI" 也算对得上）
fn slug_confident(want: &str, m: &ModrinthEnv) -> bool {
    let w = norm_key(want);
    !w.is_empty()
        && [m.slug.as_deref(), m.title.as_deref()]
            .into_iter()
            .flatten()
            .any(|s| !s.is_empty() && norm_key(s) == w)
}

/* ---------------- 项目引用推导 ---------------- */

/// Modrinth CDN URL 形如 `https://cdn.modrinth.com/data/<project_id>/versions/<vid>/<file>.jar`
pub fn project_id_from_url(url: &str) -> Option<String> {
    let pid = url.split("/data/").nth(1)?.split('/').next()?;
    (!pid.is_empty() && pid.len() <= 16).then(|| pid.to_string())
}

/// 像 Modrinth slug/id 的样子：ASCII、长度够、只含 slug 合法字符
fn is_slug(s: &str) -> bool {
    s.len() >= 3
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// 文件名能给出的 slug 候选：原样 id，外加去掉尾部加载器词的裸 id
/// （`sodium-fabric` → 先试 `sodium-fabric` 再试 `sodium`；中文文件名整体不是 slug，丢弃）
fn slugs_from_file_name(file_name: &str) -> Vec<String> {
    let (id, _) = crate::core::detector::split_mod_file(file_name);
    let id = id.trim().to_lowercase();
    let mut out = Vec::new();
    if is_slug(&id) {
        out.push(id.clone());
        if let Some((head, tail)) = id.rsplit_once('-') {
            if matches!(tail, "fabric" | "forge" | "neoforge" | "quilt" | "legacy")
                && is_slug(head)
            {
                out.push(head.to_string());
            }
        }
    }
    out
}

/// 供 commands 层组装 Target 列表（保持取证口径单一）。
/// `env_trusted` = 整包 `files[].env` 有区分度（见 detector 的同名判定）。可信时，已声明的行
/// 不必再联网：裁决表里 client/server 任一有值就能定案，重复查只会白烧 300/5min 的限流额度。
/// 全表刷成 required/required 的那种默认值包必须照查——那层声明本身就是错的。
pub fn targets_for(files: &[PackFile], env_trusted: bool) -> Vec<Target> {
    files
        .iter()
        .filter(|f| {
            !(env_trusted && (f.env_client.is_some() || f.env_server.is_some()))
        })
        .map(|f| Target {
            path: f.path.clone(),
            sha1: f.sha1.clone(),
            project_id: project_id_from_url(&f.url),
            slugs: slugs_from_file_name(&f.file_name),
            title: None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn env_of(client: SideFlag, server: SideFlag) -> ModrinthEnv {
        ModrinthEnv {
            client_side: Some(format!("{client:?}").to_lowercase()),
            server_side: Some(format!("{server:?}").to_lowercase()),
            environment: None,
            slug: Some("3dskinlayers".into()),
            title: Some("3D Skin Layers".into()),
        }
    }

    fn zip_bytes(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default();
        for (name, bytes) in files {
            w.start_file(*name, opts).unwrap();
            w.write_all(bytes).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    #[test]
    fn slug_candidates_drop_loader_suffix_and_non_ascii_names() {
        assert_eq!(
            slugs_from_file_name("sodium-fabric-0.5.13.jar"),
            vec!["sodium-fabric".to_string(), "sodium".to_string()]
        );
        assert_eq!(
            slugs_from_file_name("fabric-api-0.92.2+1.20.1.jar"),
            vec!["fabric-api".to_string()]
        );
        // 中文改名的 jar：文件名给不出任何 slug 线索（只能靠包内自报身份）
        assert!(slugs_from_file_name("万用皮肤补丁+1.5.4-mc1.20.1-fabric.jar").is_empty());
    }

    #[test]
    fn project_name_match_is_normalized_but_strict() {
        let m = env_of(SideFlag::Required, SideFlag::Unsupported);
        assert!(slug_confident("3d-skin-layers", &m));
        assert!(slug_confident("3dskinlayers", &m));
        // 差一个字都不采信：猜错项目会误删别的模组
        assert!(!slug_confident("skinlayers", &m));
    }

    #[test]
    fn search_hit_only_accepted_when_name_matches_exactly() {
        let hit = env_of(SideFlag::Required, SideFlag::Unsupported);
        let other = ModrinthEnv {
            slug: Some("something-else".into()),
            title: Some("Something Else".into()),
            ..Default::default()
        };
        let t = Target {
            path: "mods/x.jar".into(),
            sha1: None,
            project_id: None,
            slugs: vec![],
            title: Some("3D Skin Layers".into()),
        };
        assert!(pick_search_hit("3D Skin Layers", &t, &[other.clone(), hit.clone()]).is_some());
        let t2 = Target { title: Some("No Such Mod".into()), ..t.clone() };
        assert!(pick_search_hit("No Such Mod", &t2, &[other, hit]).is_none());
    }

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

    #[test]
    fn probes_fill_missing_hash_and_lead_the_slug_candidates() {
        let mut probes = HashMap::new();
        probes.insert(
            "mods/x.jar".to_string(),
            JarProbe {
                env: None,
                sha1: Some("ABCdef".into()),
                mod_id: Some("SkinLayers3D".into()),
                title: Some("3D Skin Layers".into()),
                ..JarProbe::default()
            },
        );
        let mut targets = vec![Target {
            path: "mods/x.jar".into(),
            sha1: None,
            project_id: None,
            slugs: vec!["x".into()],
            title: None,
        }];
        apply_probes(&probes, &mut targets);
        assert_eq!(targets[0].sha1.as_deref(), Some("ABCdef"));
        assert_eq!(targets[0].title.as_deref(), Some("3D Skin Layers"));
        // 模组自报 id 排在文件名猜测之前，且小写归一
        assert_eq!(targets[0].slugs, vec!["skinlayers3d", "x"]);
    }

    /* ---------------- 字节码结构事实 ---------------- */

    /// 造一个只含常量池的 class：Utf8 串按顺序放进 CP，后面接全零的结构区。
    /// 只需要常量池能被正确走完，字段/方法体都留空
    fn class_bytes(strings: &[&str]) -> Vec<u8> {
        let mut b: Vec<u8> = vec![0xCA, 0xFE, 0xBA, 0xBE, 0, 0, 0, 0];
        b.extend(((strings.len() + 1) as u16).to_be_bytes());
        for s in strings {
            b.push(1);
            b.extend((s.len() as u16).to_be_bytes());
            b.extend(s.as_bytes());
        }
        b.extend(std::iter::repeat(0u8).take(14));
        b
    }

    /// 混合定长条目的 class：Long（占两槽）、MethodHandle（3 字节）、InvokeDynamic（4 字节）
    /// 各一个，再跟一个 Utf8。宽度算错的话尾部会解不出标记
    fn class_bytes_mixed() -> Vec<u8> {
        let mut b: Vec<u8> = vec![0xCA, 0xFE, 0xBA, 0xBE, 0, 0, 0, 0];
        b.extend(6u16.to_be_bytes()); // Long 占两槽 → Utf8 落在槽位 5，count 得写到 6
        b.push(5); // Long
        b.extend(std::iter::repeat(0u8).take(8));
        b.push(15); // MethodHandle
        b.extend([1, 0, 2]);
        b.push(18); // InvokeDynamic
        b.extend([0, 3, 0, 4]);
        b.push(1);
        let s = b"Lnet/minecraftforge/fml/event/lifecycle/FMLCommonSetupEvent;";
        b.extend((s.len() as u16).to_be_bytes());
        b.extend(s);
        b.extend(std::iter::repeat(0u8).take(14));
        b
    }

    fn jar_of(classes: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let refs: Vec<(&str, &[u8])> = classes
            .iter()
            .map(|(n, b)| (*n, b.as_slice()))
            .collect();
        zip_bytes(&refs)
    }

    #[test]
    fn code_facts_flag_server_registration_but_not_rendering_code() {
        let server = jar_of(&[(
            "com/x/Setup.class",
            class_bytes(&[
                "com/x/Setup",
                "Lnet/minecraftforge/fml/event/lifecycle/FMLCommonSetupEvent;",
            ]),
        )]);
        let f = read_code_facts(&server);
        assert!(f.server_code);
        assert!(!f.client_only_shape, "有服务端注册时不该再标纯客户端形状");

        // 纯客户端模组照样大量引用 world/entity 类：这类前缀不参与判定
        let client_only_mod = jar_of(&[(
            "com/x/Hud.class",
            class_bytes(&[
                "com/x/Hud",
                "net/minecraft/world/entity/LivingEntity",
                "net/minecraft/client/gui/GuiGraphics",
            ]),
        )]);
        let f = read_code_facts(&client_only_mod);
        assert!(!f.server_code);
        assert!(!f.client_only_shape, "只引用了 MC 类、没有任何注册 → 形状也不下结论");

        // 只见客户端生命周期注册 → 记为「形状像纯客户端」
        let client_setup = jar_of(&[(
            "com/x/Client.class",
            class_bytes(&[
                "com/x/Client",
                "Lnet/minecraftforge/fml/client/event/FMLClientSetupEvent;",
            ]),
        )]);
        let f = read_code_facts(&client_setup);
        assert!(!f.server_code);
        assert!(f.client_only_shape);
    }

    #[test]
    fn utf8_walker_survives_variable_slot_constant_pool_entries() {
        let jar = jar_of(&[("com/x/Mixed.class", class_bytes_mixed())]);
        assert!(read_code_facts(&jar).server_code);
    }

    #[test]
    fn code_facts_stop_at_versioned_classes_and_survive_garbage() {
        // META-INF/versions/ 下的多版本副本不代表这个 jar 的注册行为
        let jar = jar_of(&[
            ("com/x/Main.class", class_bytes(&["com/x/Main"])),
            (
                "META-INF/versions/21/com/x/Main.class",
                class_bytes(&["com/x/Main", "RegisterCapabilitiesEvent"]),
            ),
        ]);
        let f = read_code_facts(&jar);
        assert!(!f.server_code);
        // 非 class 条目与坏字节：静默无结论，不 panic
        assert_eq!(read_code_facts(&zip_bytes(&[("a.txt", b"hi".as_slice())])), f);
        assert_eq!(read_code_facts(b"not a zip at all"), CodeFacts::default());
        let mut truncated = class_bytes(&["java/lang/Object", "DeferredRegister"]);
        truncated.truncate(20);
        assert!(
            !read_code_facts(&jar_of(&[("com/x/T.class", truncated)])).server_code,
            "半截 class 不能解出错位结论"
        );
    }
}
