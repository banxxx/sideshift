//! 第 3/4 层：在线反查（按设置只走 Modrinth 官方或麦块镜像那一家）+ `cache_dir/env-index.json`
//! 本地索引（离线即答）。

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::Duration;

use futures::stream::{self, StreamExt};
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
///
/// `transparent`：盘上存的就是这张表本身（`save` 写 `&self.map`），不是 `{"map": …}` 的包装。
/// 少了这一句，读回来时外层字段对不上、整表被 serde 当未知键丢掉 ⇒ **写进去的取证永远读不出来**，
/// 每一轮都从零发请求（离线即答形同虚设，也正是「有时自动分类特别久」的一条真凶）
#[derive(Default, Deserialize)]
#[serde(transparent)]
pub struct EnvIndex {
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
/// 免得一次转换打满 Modrinth 限流（300 req/min）影响后续下载。
/// 限流按**总请求数**算，所以下面那点并发只压时间、不抬高总量，与这一档不冲突。
const MAX_LOOKUP_REQUESTS: usize = 150;

/// 项目/搜索反查的并发宽度：这些行互不依赖，串行时一个慢请求就拖住整队（整轮慢的主因）。
/// 4 路对齐 jar 扫描的线程数，也够把上限内的 150 次查询从分钟级压到十秒级。
const LOOKUP_CONCURRENCY: usize = 4;

/// 第 4 层每这么多行跑一批：批内并发、**批末落一次盘**。整轮随时可能被命令层的墙钟预算
/// 掐掉，攒到最后才写 `env-index.json` 等于那一截白查（下次进来又从零要请求）。
const LOOKUP_CHUNK: usize = 32;

/// 镜像档两条腿各自的整轮请求上限。对方公布 **600 次/分钟**，一轮最多 2（自检）+150+150=302 发，
/// 留一半余量给同一轮的模组下载。
/// 项目腿与搜索腿**各占一半**而不是共用一条流水账：项目腿按整条候选链预留，
/// 150 行就能吃满 300，后跑的搜索腿会常年分到 0——平分反而更好预测。
/// 撞顶不报失败（预算本来就是设计上限）；但**自检不过算失败**——这一档镜像就是被选中的源，
/// 没有官方链可回落。
const MIRROR_PROJECT_BUDGET: usize = 150;
const MIRROR_SEARCH_BUDGET: usize = 150;

/// 在线反查整轮的墙钟预算。命令层用它掐 `resolve_online`：单次请求已经有
/// `downloader::client::METADATA_TIMEOUT` 各兜 10s，这一档管的是「一百多个请求各慢一点」
/// 累出来的总账——到点就带着已拿到的部分结论收场，前端按「未全部完成」提示重新分类。
pub const ONLINE_BUDGET: Duration = Duration::from_secs(60);

/// 第 4 层的行队列：预算分配 + 候选链。
///
/// 预算按**整条候选链**预留（权威 project_id 在前、猜出的 slug 在后），装不下的行整批不查：
/// 与串行版「撞顶后 break、剩下的行一条没查」同判据，只是这里提前算出来了。
/// 返回 `(待查队列, 压根没候选的行, 预留请求数, 是否撞到上限)`。
fn project_queue(
    targets: &[Target],
    unresolved: &[usize],
    max: usize,
) -> (Vec<(usize, Vec<(String, bool)>)>, Vec<usize>, usize, bool) {
    let mut queue = Vec::new();
    let mut no_cand = Vec::new();
    let mut used = 0usize;
    let mut capped = false;
    for &i in unresolved {
        let t = &targets[i];
        let cands: Vec<(String, bool)> = t
            .project_id
            .as_deref()
            .map(|k| (k.to_string(), true))
            .into_iter()
            .chain(t.slugs.iter().map(|k| (k.clone(), false)))
            .collect();
        if cands.is_empty() {
            no_cand.push(i);
            continue;
        }
        let room = max - used;
        if room == 0 {
            capped = true;
            continue;
        }
        let take = cands.len().min(room);
        if take < cands.len() {
            capped = true;
        }
        used += take;
        queue.push((i, cands.into_iter().take(take).collect()));
    }
    (queue, no_cand, used, capped)
}

/// 镜像项目层的行队列：与 `project_queue` 同口径（整条候选链一起预留、装不下的行本轮不派），
/// 但**只喂 slug 候选**——实测镜像只认 slug，官方那套 `project_id` 形式回 404。
/// 这里刻意不报「撞顶」：预算上限是设计里的，剩下的行本轮没有结论、下轮再查，不算失败。
fn mirror_project_queue(
    targets: &[Target],
    unresolved: &[usize],
    max: usize,
) -> Vec<(usize, Vec<String>)> {
    let mut queue = Vec::new();
    let mut used = 0usize;
    for &i in unresolved {
        let slugs = &targets[i].slugs;
        if slugs.is_empty() {
            continue;
        }
        let room = max.saturating_sub(used);
        if room == 0 {
            break;
        }
        let take = slugs.len().min(room);
        used += take;
        queue.push((i, slugs.iter().take(take).cloned().collect()));
        if take < slugs.len() {
            break;
        }
    }
    queue
}

/// 在线反查：**设置里选了哪个源就只问那个源**，不再「先镜像、答不上回落官方」——
/// 那样两家的钱都付一遍，选了国内源反而比纯官方更慢（镜像 0.2s + 官方 2.2s 串在一行上）。
/// `mirror=true` ⇒ 整条链只发 `api.minekuai.cn`（含 sha1 批量那条 Modrinth-only 接口也不发，
/// 见 `resolve_via_mirror` 的代价说明）；`mirror=false` ⇒ 官方三条腿照旧，镜像连存活自查都不做。
/// 结果同时写回 out 与索引，并且分批落盘。
/// 返回 false = 有请求失败、超出请求上限，或被整轮预算掐掉（前端提示「联网反查未全部完成」）。
pub async fn resolve_online(
    dl: &Downloader,
    index: &mut EnvIndex,
    cache_dir: &Path,
    targets: &[Target],
    pending: &[usize],
    out: &mut EvidenceMap,
    mirror: bool,
) -> bool {
    if mirror {
        return resolve_via_mirror(dl, index, cache_dir, targets, pending, out).await;
    }
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
    // 这一层的命中是整轮里最值钱的（精确到文件），后面第 4 层再怎么被掐也不该带走它
    index.save(cache_dir);

    // 第 4 层：项目级 client_side / server_side（哈希不在 Modrinth 上的条目、裸 zip 条目）
    let (queue, mut by_name, requests, capped) =
        project_queue(targets, &unresolved, MAX_LOOKUP_REQUESTS);
    if capped {
        ok = false;
    }
    for chunk in queue.chunks(LOOKUP_CHUNK) {
        // `to_vec()`：让流走拥有值。按引用喂 `buffer_unordered` 会把 `map` 的 closure 绑死在
        // 一个寿命上，`spawn` 那条链上报 `FnOnce is not general enough`
        let mut done: Vec<(usize, Option<(Vec<String>, Evidence)>, bool)> =
            stream::iter(chunk.to_vec())
                .map(|(i, cands)| async move {
                    let mut failed = false;
                    let mut got = None;
                    for (key, authoritative) in cands {
                        // 权威（URL 带出的 project_id）查到即采信；slug 候选是猜的，
                        // 返回项目的 slug/title 与候选对得上才采信（猜错项目比漏判更糟：会误删别的模组）
                        match dl.project_env(&key).await {
                            Ok(Some(m)) => {
                                if !authoritative && !slug_confident(&key, &m) {
                                    continue;
                                }
                                if let Some(ev) =
                                    evidence_from_modrinth(&m, EnvSource::ModrinthProject)
                                {
                                    got = Some((resolved_key(&m, &key), ev));
                                    break;
                                }
                            }
                            // Ok(None) = 404，项目不存在：不是故障，换下一个候选
                            Ok(None) => {}
                            Err(_) => failed = true,
                        }
                    }
                    (i, got, failed)
                })
                .buffer_unordered(LOOKUP_CONCURRENCY)
                .collect()
                .await;
        // 完成序不保证：排回行序，同一项目键被两行命中时落盘的结论才不随网络抖
        done.sort_by_key(|(i, _, _)| *i);
        for (i, got, failed) in done {
            if failed {
                ok = false;
            }
            let t = &targets[i];
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
        index.save(cache_dir);
    }

    // 第 4 层补充：按模组显示名走 /v2/search（命中项自带 client_side/server_side/environment）
    let queries: Vec<(usize, String)> = {
        by_name.sort_unstable();
        by_name
            .iter()
            .filter_map(|&i| {
                let t = &targets[i];
                t.title
                    .as_deref()
                    .or(t.slugs.first().map(|s| s.as_str()))
                    .map(|q| (i, q.to_string()))
            })
            .collect()
    };
    // 一行一次请求，剩多少预算发多少；剩下的行不查（与串行版撞顶 break 同判据）。
    // 官方档这条腿只发 `api.modrinth.com`：镜像那条腿在 `resolve_via_mirror` 里，两边不同时跑。
    let run = queries.len().min(MAX_LOOKUP_REQUESTS.saturating_sub(requests));
    if run < queries.len() {
        ok = false;
    }
    for chunk in queries[..run].chunks(LOOKUP_CHUNK) {
        let mut done: Vec<(usize, Option<(Vec<String>, Evidence)>, bool)> =
            stream::iter(chunk.to_vec())
                .map(|(i, query)| async move {
                    let q = query.as_str();
                    match dl.search_env(q).await {
                        Ok(list) => (
                            i,
                            pick_search_hit(q, &targets[i], &list).and_then(|m| {
                                evidence_from_modrinth(m, EnvSource::ModrinthProject)
                                    .map(|ev| (resolved_key(m, q), ev))
                            }),
                            // 搜得到但名字对不上：不采信，也不算故障
                            false,
                        ),
                        Err(_) => (i, None, true),
                    }
                })
                .buffer_unordered(LOOKUP_CONCURRENCY)
                .collect()
                .await;
        done.sort_by_key(|(i, _, _)| *i);
        for (i, got, failed) in done {
            if failed {
                ok = false;
            }
            let t = &targets[i];
            if let Some((keys, ev)) = got {
                put(out, &t.path, ev);
                for k in keys {
                    index.record(project_key(&k), ev);
                }
            }
        }
        index.save(cache_dir);
    }

    index.save(cache_dir);
    ok
}

/// 麦块档的整条联网链：**只发 `api.minekuai.cn`**，官方三条腿（sha1 批量 / 项目 / 搜索）一条都不碰。
///
/// 两条入口各发一发自检后才派腿（路径被改名时它回的是 200 + `{"code":404}`，只看状态码会被骗）。
/// 这一档里镜像就是**用户选中的那个源**，不是加速件 ⇒ 自检验不过就等于这一轮没跑完（`ok=false`，
/// 前端如实提示「联网反查未全部完成」），不再像旧口径那样悄悄回落官方。
///
/// **这一档拿不到的东西**（设置项的说明文案要对得上）：
/// - 第 3 层「按 sha1 批量反查构建」（精确到这一个文件的 `environment`，rank 1）：它没有对应端点；
/// - 权威 `project_id` 那条候选（URL 里带出的那个）：它只认 slug，`detail/u6dRKJwZ` 实测 404；
/// - 不在收录的模组（抽样的 `coppered-equipment` 就 404）：这一轮没有第二个源可问，停在「未判定」。
///
/// 落盘的结论一律标 `MirrorProject`（rank 3），阶梯与官方档同一条：已经在索引里的 jar/hash 级证据
/// 不会因为换档被压掉（`put` 只认等档或更好档）。
async fn resolve_via_mirror(
    dl: &Downloader,
    index: &mut EnvIndex,
    cache_dir: &Path,
    targets: &[Target],
    pending: &[usize],
    out: &mut EvidenceMap,
) -> bool {
    let health = dl.mirror_health().await;
    let mut ok = health.project && health.search;
    let mut answered: HashSet<usize> = HashSet::new();

    // 项目腿：候选全是猜的（镜像没有权威 id 那条路），所以一律要名字对得上才算采信
    if health.project {
        let queue = mirror_project_queue(targets, pending, MIRROR_PROJECT_BUDGET);
        for chunk in queue.chunks(LOOKUP_CHUNK) {
            let mut done: Vec<(usize, Option<(Vec<String>, Evidence)>, bool)> =
                stream::iter(chunk.to_vec())
                    .map(|(i, slugs)| async move {
                        let mut got = None;
                        // 真 404 / 200+code:404 = 不在收录，那是「问到了、它没有」，不算失败；
                        // 网络故障要记：这一档没有官方链兜着，一行没问到就是这一轮没跑完
                        let mut failed = false;
                        for key in slugs {
                            match dl.project_env_via_mirror(&key).await {
                                Ok(Some(m)) => {
                                    if !slug_confident(&key, &m) {
                                        continue;
                                    }
                                    if let Some(ev) =
                                        evidence_from_modrinth(&m, EnvSource::MirrorProject)
                                    {
                                        got = Some((resolved_key(&m, &key), ev));
                                        break;
                                    }
                                }
                                Ok(None) => {}
                                Err(_) => failed = true,
                            }
                        }
                        (i, got, failed)
                    })
                    .buffer_unordered(LOOKUP_CONCURRENCY)
                    .collect()
                    .await;
            // 完成序不保证：排回行序，同一项目键被两行命中时落盘的结论才不随网络抖
            done.sort_by_key(|(i, _, _)| *i);
            for (i, got, failed) in done {
                if failed {
                    ok = false;
                }
                let Some((keys, ev)) = got else { continue };
                let t = &targets[i];
                put(out, &t.path, ev);
                for k in keys {
                    index.record(project_key(&k), ev);
                }
                answered.insert(i);
            }
            index.save(cache_dir);
        }
    }

    // 搜索腿：项目腿没答上的行按显示名/首个 slug 再问一次，一行一发。
    // 采信规则与官方那条逐字相同（只认 slug / 英文名严格同形）：`ModrinthEnv` 里没有 `title_zh` 可比，
    // 所以中文标题那一档两边都答不上，别把这条腿当中文检索的出口
    if health.search {
        let rest: Vec<usize> = {
            let mut v: Vec<usize> = pending
                .iter()
                .copied()
                .filter(|i| !answered.contains(i))
                .collect();
            v.sort_unstable();
            v
        };
        let queries: Vec<(usize, String)> = rest
            .iter()
            .filter_map(|&i| {
                let t = &targets[i];
                t.title
                    .as_deref()
                    .or(t.slugs.first().map(|s| s.as_str()))
                    .map(|q| (i, q.to_string()))
            })
            .collect();
        // 装不下的行本轮不查：这一档没有第二个源可回落，但预算掐顶是设计上限、不是故障，不报 ok=false
        let run = queries.len().min(MIRROR_SEARCH_BUDGET);
        for chunk in queries[..run].chunks(LOOKUP_CHUNK) {
            let mut done: Vec<(usize, Option<(Vec<String>, Evidence)>, bool)> =
                stream::iter(chunk.to_vec())
                    .map(|(i, query)| async move {
                        let q = query.as_str();
                        let (list, failed) = match dl.search_env_via_mirror(q).await {
                            Ok(list) => (list, false),
                            Err(_) => (Vec::new(), true),
                        };
                        (
                            i,
                            pick_search_hit(q, &targets[i], &list).and_then(|m| {
                                evidence_from_modrinth(m, EnvSource::MirrorProject)
                                    .map(|ev| (resolved_key(m, q), ev))
                            }),
                            failed,
                        )
                    })
                    .buffer_unordered(LOOKUP_CONCURRENCY)
                    .collect()
                    .await;
            done.sort_by_key(|(i, _, _)| *i);
            for (i, got, failed) in done {
                if failed {
                    ok = false;
                }
                let Some((keys, ev)) = got else { continue };
                let t = &targets[i];
                put(out, &t.path, ev);
                for k in keys {
                    index.record(project_key(&k), ev);
                }
            }
            index.save(cache_dir);
        }
    }

    index.save(cache_dir);
    ok
}

/// 单个本地 jar 的端取证阶梯（用户手动添加的行走这条路），口径与整包分类完全一致：
/// jar 自证 → 本地索引（离线即答）→ 联网按设置选定的那一个源反查并落盘（见 `resolve_online`）。
/// 返回 `None` = 三层都没结论（前端标「需人工确认」，绝不猜）
pub async fn resolve_local_jar(
    dl: &Downloader,
    cache_dir: &Path,
    file_name: &str,
    probe: &JarProbe,
    online: bool,
    mirror: bool,
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
        resolve_online(
            dl, &mut index, cache_dir, &targets, &pending, &mut out, mirror,
        )
        .await;
    }
    out.get(&key).copied()
}

/// 「从网络添加」的构建行走同一条阶梯。CurseForge 的响应里没有任何端声明（实测 file 对象
/// 只有 `gameVersions` 那种类标签），但它给了**构建字节的 sha1**——同一份 jar 传到两个平台
/// 哈希逐字相同，所以拿它当身份就能问到 Modrinth 那两层（按哈希反查构建 → 项目/显示名），
/// 结论与包内行同口径、同样落盘，第二次添加零请求。
/// 与 `resolve_local_jar` 的差别只有 probe 是拼出来的：没有 jar 字节可解，
/// 所以 `env`（作者自证）与 `code`（字节码提示）恒空，只剩平台那几层可走。
/// `sha1` 只收 40 位 hex：拿到脏值就当没有，免得往索引里写一个永远对不上的键。
pub async fn resolve_added_build(
    dl: &Downloader,
    cache_dir: &Path,
    file_name: &str,
    sha1: Option<&str>,
    title: Option<&str>,
    online: bool,
    mirror: bool,
) -> Option<Evidence> {
    let probe = JarProbe {
        sha1: sha1
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| s.len() == 40 && s.chars().all(|c| c.is_ascii_hexdigit())),
        title: title.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
        ..Default::default()
    };
    resolve_local_jar(dl, cache_dir, file_name, &probe, online, mirror).await
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

    fn row(slugs: &[&str]) -> Target {
        Target {
            path: "mods/x.jar".into(),
            sha1: None,
            project_id: None,
            slugs: slugs.iter().map(|s| s.to_string()).collect(),
            title: None,
        }
    }

    #[test]
    fn lookup_budget_never_overshoots_and_says_so() {
        // 50 行 × 2 个候选 = 100 次请求，预算只给 5：预留按整条链，装不下的行整批不查
        let targets: Vec<Target> = (0..50)
            .map(|n| row(&[&format!("m{n}a"), &format!("m{n}b")]))
            .collect();
        let all: Vec<usize> = (0..targets.len()).collect();
        let (queue, no_cand, used, capped) = project_queue(&targets, &all, 5);
        assert_eq!(used, 5, "预留数不能超发：超了就是在跟 Modrinth 的限流赌");
        assert!(capped, "还剩 47 行没查，必须报未全部完成");
        assert!(no_cand.is_empty());
        // 前 2 行整链进队，第 3 行只分到 1 个候选，后面的行一条没发
        assert_eq!(queue[0].1.len(), 2);
        assert_eq!(queue[1].1.len(), 2);
        assert_eq!(queue[2].1, vec![("m2a".to_string(), false)]);
        assert_eq!(queue.len(), 3);
    }

    #[test]
    fn rows_with_no_project_candidate_cost_nothing_and_are_not_capped() {
        // 裸 zip 又推不出 slug：不占预算，直接交给按名搜索那一层
        let targets = vec![row(&["fine"]), row(&[]), row(&[])];
        let all: Vec<usize> = vec![0, 1, 2];
        let (queue, no_cand, used, capped) = project_queue(&targets, &all, 150);
        assert_eq!(queue.len(), 1);
        assert_eq!(no_cand, vec![1, 2]);
        assert_eq!(used, 1);
        assert!(!capped);
    }

    #[test]
    fn mirror_queue_only_takes_slug_candidates_and_never_complains() {
        // 镜像只认 slug：project_id 那条权威候选压根不在这一步的输入里（Target 也没带）
        let targets = vec![row(&["a", "b"]), row(&[]), row(&["c"])];
        let all: Vec<usize> = vec![0, 1, 2];
        assert_eq!(
            mirror_project_queue(&targets, &all, 150),
            vec![
                (0usize, vec!["a".to_string(), "b".to_string()]),
                (2usize, vec!["c".to_string()])
            ],
            "没候选的行跳过就行：它照旧走官方，不算镜像的失败"
        );
        // 预算只够第 0 行的第一个候选 ⇒ 该行只派一发，后面的行本轮一条不发
        assert_eq!(
            mirror_project_queue(&targets, &all, 1),
            vec![(0usize, vec!["a".to_string()])]
        );
    }

    #[tokio::test]
    #[ignore = "真联网：镜像两条腿 + 官方回落各发几发请求"]
    async fn mirror_round_answers_labels_itself_and_404_is_not_a_failure() {
        // 这一条测的是「加速件」的全部契约，少一条都不算通：
        // ① 存活自查过后，在册的行由镜像答上（来源标 MirrorProject，不冒充平台项目）
        // ② 镜像答上的行不再发官方腿（不然这一档白加）
        // ③ 两边都查不到的行留空，且 `complete` 仍然是 true——404 不是故障
        let dir = std::env::temp_dir().join(format!(
            "sideshift-env-mirror-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let _ = std::fs::remove_file(dir.join(INDEX_FILE));
        let targets: Vec<Target> = ["sodium", "xaeros-world-map", "zz-not-a-real-mod"]
            .iter()
            .map(|slug| Target {
                path: format!("mods/{slug}.jar"),
                sha1: None,
                project_id: None,
                slugs: vec![slug.to_string()],
                title: None,
            })
            .collect();
        let all: Vec<usize> = vec![0, 1, 2];
        let mut index = EnvIndex::load(&dir);
        let mut out = EvidenceMap::new();
        let dl = Downloader::new(dir.clone(), 4);
        let ok = resolve_online(
            &dl,
            &mut index,
            &dir,
            &targets,
            &all,
            &mut out,
            true,
        )
        .await;

        assert!(ok, "整轮都跑通了却报「未全部完成」= 把 404 当故障，或镜像侧的失败被算了进去");
        let sodium = out.get("mods/sodium.jar").copied();
        assert_eq!(
            sodium.map(|e| e.source),
            Some(EnvSource::MirrorProject),
            "sodium 在镜像在册却没走镜像腿 ⇒ 前置那一层没生效"
        );
        assert_eq!(
            sodium.and_then(|e| e.server),
            Some(SideFlag::Unsupported),
            "官方与镜像的端声明必须一致，否则测的是两家数据在漂"
        );
        assert_eq!(
            out.get("mods/xaeros-world-map.jar").map(|e| e.source),
            Some(EnvSource::MirrorProject),
            "slug 与返回 title 写法不同（Xaero's World Map）：归一化同形就该采信"
        );
        assert!(
            !out.contains_key("mods/zz-not-a-real-mod.jar"),
            "查无此模组必须留空待人工，绝不拿名字猜一个"
        );
        let _ = std::fs::remove_dir_all(&dir);
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
        let ev = resolve_local_jar(
            &dl,
            &dir,
            "obscure-lib-1.0.jar",
            &probe_local_jar(&path),
            false,
            false,
        )
        .await;
        assert!(ev.is_none(), "离线无证据时必须留空，不能编一个来源");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 往索引里预置一条结论并**真的落盘**：测的就是下一次冷启动读到的那份 `env-index.json`
    fn seed_index(dir: &Path, key: String) {
        let ev = evidence_from_modrinth(
            &env_of(SideFlag::Required, SideFlag::Unsupported),
            EnvSource::ModrinthHash,
        )
        .expect("required/unsupported 有区分度");
        let mut index = EnvIndex::default();
        index.record(key, ev);
        index.save(dir);
    }

    #[test]
    fn index_round_trips_through_its_own_file() {
        // 落盘的形状必须与读回的解析对得上：`save` 写的是**裸表**（`{"sha1:…": …}`），
        // 外层曾多包一层 `map` 去解析 ⇒ 键全被 serde 当未知字段丢掉，取证永远读不回来，
        // 每一轮都从零发请求。「离线即答」这条承诺整个挂在这一对上
        let dir = std::env::temp_dir().join(format!(
            "sideshift-index-roundtrip-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let key = sha1_key("bb0cb397083a0be2601bd4c6f7060326a1c8505f");
        seed_index(&dir, key.clone());
        let back = EnvIndex::load(&dir);
        assert_eq!(
            back.get(&key).map(|e| (e.source, e.client, e.server)),
            Some((
                EnvSource::ModrinthHash,
                Some(SideFlag::Required),
                Some(SideFlag::Unsupported)
            )),
            "写进去的取证必须能读回来"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn added_build_answers_offline_from_its_sha1() {
        // 「从网络添加」的 CurseForge 构建没有任何端声明，但它给的 sha1 与 Modrinth 那份
        // 字节逐字相同 ⇒ 索引里挂过这份哈希的结论，离线就该直接答上（大写输入照样归一）
        let dir = std::env::temp_dir().join(format!(
            "sideshift-added-build-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let sha1 = "bb0cb397083a0be2601bd4c6f7060326a1c8505f";
        seed_index(&dir, sha1_key(sha1));
        let got = resolve_added_build(
            &Downloader::new(dir.clone(), 1),
            &dir,
            "jei-1.20.1-forge-15.20.0.105.jar",
            Some(&sha1.to_uppercase()),
            Some("Just Enough Items (JEI)"),
            false,
            false,
        )
        .await
        .expect("索引答过这份哈希，离线就该答");
        // `Evidence` 没有 PartialEq（它只是三个 Option 的组合，比较在裁决层）⇒ 逐字段对
        assert_eq!(got.source, EnvSource::ModrinthHash);
        assert_eq!(got.client, Some(SideFlag::Required));
        assert_eq!(got.server, Some(SideFlag::Unsupported));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn added_build_trusts_only_a_real_sha1() {
        // 脏值不当身份用：少一位、非 hex 都算没拿到哈希。认了它等于拿一个字符串当主键，
        // 两个模组共用同一个脏值就会互相顶包（这里索引里正好有那条脏键，答上就是错的）
        let dir = std::env::temp_dir().join(format!(
            "sideshift-added-dirty-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        seed_index(&dir, sha1_key("not-a-real-hash"));
        for bad in [
            "not-a-real-hash",
            "bb0cb397083a0be2601bd4c6f7060326a1c8505",
            "zz0cb397083a0be2601bd4c6f7060326a1c8505f",
        ] {
            let ev = resolve_added_build(
                &Downloader::new(dir.clone(), 1),
                &dir,
                "obscure-lib-1.0.jar",
                Some(bad),
                None,
                false,
                false,
            )
            .await;
            assert!(ev.is_none(), "脏哈希 {bad} 不许对上任何结论");
        }
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
