//! 端判定证据采集：判定一个模组「服务端要不要」的取证层，不含裁决规则（裁决在 detector）。
//!
//! 证据阶梯（可信度由高到低，detector 依此取第一层命中的）：
//! 1. `mrpack files[].env`——整合包作者显式声明，parser 直接给出，不在本模块
//! 2. 包内 jar 的 `fabric.mod.json:environment`——模组作者自证（本模块离线扫描）
//! 3. Modrinth 按文件 sha1 反查构建的 `environment`——平台侧、精确到这一个文件
//!    （裸 zip 的 index 没给哈希 → 本模块扫描时自行计算 jar 字节 sha1）
//! 4. Modrinth 项目级 `client_side/server_side`——按文件名/包内自报 id 推出的 slug，
//!    名字对得上才采信；再兜不住按模组显示名走 `/v2/search`（第 4 层内，同样标平台项目）
//! 5. detector 的模组名关键字表——纯猜，兜底
//!
//! 第 3、4 层是网络查询，结果落 `cache_dir/env-index.json`：同一个模组第二次见到即离线可答。
//! Forge / NeoForge 的 `META-INF/mods.toml` 没有自证端字段（实测），所以第 2 层只覆盖 Fabric / Quilt；
//! 字节码扫描 `net/minecraft/client` 同样不可用（官方重混淆后命中率 0），故不做。
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

fn rank(s: EnvSource) -> u8 {
    match s {
        EnvSource::Mrpack => 0,
        EnvSource::JarMetadata => 1,
        EnvSource::ModrinthHash => 2,
        EnvSource::ModrinthProject => 3,
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
}

/// 一条离线探测请求。`want_sha1` 关掉时不整包算哈希——mrpack 已在 index 里给了 sha1，
/// 再算一遍等于白读几百 MB；裸 zip 与手动塞入的 jar 才需要我们自己算（第 3 层的入场券）
#[derive(Clone, Debug)]
pub struct ProbeReq {
    pub path: String,
    pub want_sha1: bool,
}

/// 离线扫描源包内指定 jar 条目。读不到一律静默跳过（交上下层）。
pub fn probe_jars(pack: &Path, want: &[ProbeReq]) -> HashMap<String, JarProbe> {
    let mut out = HashMap::new();
    let want_map: HashMap<&str, bool> = want
        .iter()
        .map(|w| (w.path.as_str(), w.want_sha1))
        .collect();
    let Ok(file) = File::open(pack) else {
        return out;
    };
    let Ok(archive) = zip::ZipArchive::new(file) else {
        return out;
    };
    let targets: Vec<(usize, String, bool)> = archive
        .file_names()
        .enumerate()
        .filter_map(|(i, name)| {
            want_map.get(name).map(|hash| (i, name.to_string(), *hash))
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
                let chunk: Vec<(usize, String, bool)> = chunk.to_vec();
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
fn jar_chunk(pack: &Path, chunk: &[(usize, String, bool)]) -> Vec<(String, JarProbe)> {
    let mut out = Vec::new();
    let Ok(file) = File::open(pack) else {
        return out;
    };
    let Ok(mut archive) = zip::ZipArchive::new(file) else {
        return out;
    };
    for (idx, path, want_sha1) in chunk {
        if let Some(probe) = jar_probe(&mut archive, *idx, *want_sha1) {
            out.push((path.clone(), probe));
        }
    }
    out
}

fn jar_probe<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    idx: usize,
    want_sha1: bool,
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
            if env.is_empty() {
                continue;
            }
            let (client, server) = match env.trim().to_lowercase().as_str() {
                "client" => (SideFlag::Required, SideFlag::Unsupported),
                "server" => (SideFlag::Unsupported, SideFlag::Required),
                "*" => (SideFlag::Optional, SideFlag::Optional),
                _ => continue,
            };
            if probe.env.is_none() {
                probe.env = Some(Evidence {
                    client: Some(client),
                    server: Some(server),
                    source: EnvSource::JarMetadata,
                });
            }
        } else {
            // Forge / NeoForge 的 mods.toml：无端字段，只有身份可取
            set_if_empty(&mut probe.mod_id, &toml_field(&text, "modId"));
            set_if_empty(&mut probe.title, &toml_field(&text, "displayName"));
        }
    }
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
/// mrpack 已声明任一端的行不再联网：裁决表里 server 或 client 任一有值都能定案，
/// 再查一遍只会白烧 Modrinth 的限流额度（大包一次能打掉上百请求）。
pub fn targets_for(files: &[PackFile]) -> Vec<Target> {
    files
        .iter()
        .filter(|f| f.env_client.is_none() && f.env_server.is_none())
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
    fn probe_jars_reads_self_declared_env_sha1_and_identity() {
        let jar = zip_bytes(&[(
            "fabric.mod.json",
            br#"{"id":"skinlayers3d","name":"3D Skin Layers","environment":"client"}"#,
        )]);
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
        // 只要元数据的请求（mrpack 已给哈希）不该产出 sha1——那是白读整包
        let light = probe_jars(
            &path,
            &[ProbeReq {
                path: "mods/万用皮肤补丁.jar".to_string(),
                want_sha1: false,
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
}
