//! 第 3/4 层：Modrinth 反查（在线）+ `cache_dir/env-index.json` 本地索引（离线即答）。

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

use crate::core::downloader::{Downloader, ModrinthEnv};
use crate::models::EnvSource;
use super::evidence::{evidence_from_modrinth, put, Evidence, EvidenceMap};
use super::ident::{is_slug, slugs_from_file_name};
use super::jar::JarProbe;

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

/// 单个本地 jar 的端取证阶梯（用户手动添加的行走这条路），口径与整包分类完全一致：
/// jar 自证 → 本地索引（离线即答）→ 联网按 sha1/项目/显示名反查并落盘。
/// 返回 `None` = 三层都没结论（前端标「需人工确认」，绝不猜）
pub async fn resolve_local_jar(
    dl: &Downloader,
    cache_dir: &Path,
    file_name: &str,
    probe: &JarProbe,
    online: bool,
) -> Option<Evidence> {
    let key = file_name.to_string();
    let mut out: EvidenceMap = HashMap::new();
    if let Some(ev) = probe.env {
        put(&mut out, &key, ev);
    }
    let mut targets = vec![Target {
        path: key.clone(),
        sha1: probe.sha1.clone(),
        project_id: None,
        slugs: slugs_from_file_name(file_name),
        title: None,
    }];
    let mut probes = HashMap::new();
    probes.insert(key.clone(), probe.clone());
    apply_probes(&probes, &mut targets);
    let mut index = EnvIndex::load(cache_dir);
    let pending = apply_index(&index, &targets, &mut out);
    if online && !pending.is_empty() {
        resolve_online(dl, &mut index, cache_dir, &targets, &pending, &mut out).await;
    }
    out.get(&key).copied()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::env::fixtures::zip_bytes;
    use crate::core::env::jar::probe_local_jar;
    use crate::models::SideFlag;

    fn env_of(client: SideFlag, server: SideFlag) -> ModrinthEnv {
        ModrinthEnv {
            client_side: Some(format!("{client:?}").to_lowercase()),
            server_side: Some(format!("{server:?}").to_lowercase()),
            environment: None,
            slug: Some("3dskinlayers".into()),
            title: Some("3D Skin Layers".into()),
        }
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

    #[tokio::test]
    async fn local_jar_without_any_side_proof_stays_undecided_offline() {
        // Forge 的 mods.toml 没有端字段（实测）：离线三层全空时返回 None，
        // 由前端标「需人工确认」——绝不拿文件名猜一个新添加的模组
        let jar = zip_bytes(&[(
            "META-INF/mods.toml",
            br#"modId = "obscure-lib"
displayName = "Obscure Lib"
"#,
        )]);
        let dir = std::env::temp_dir().join(format!(
            "sideshift-env-unknown-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("obscure-lib-1.0.jar");
        std::fs::write(&path, &jar).unwrap();

        let dl = Downloader::new(dir.clone(), 1);
        let ev =
            resolve_local_jar(&dl, &dir, "obscure-lib-1.0.jar", &probe_local_jar(&path), false)
                .await;
        assert!(ev.is_none(), "离线无证据时必须留空，不能编一个来源");
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
}
