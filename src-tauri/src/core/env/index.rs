//! 第 3/4 层：在线反查（按 `env_lookup_source` 那一档只走 Modrinth 官方或麦块镜像那一家）
//! + `cache_dir/env-index.json` 本地索引（离线即答）+ CF 指纹腿（官方平台、凭用户自己的 Key）。

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};

use crate::core::downloader::{mcmod_confident, Downloader, McmodPage, ModrinthEnv};
use crate::models::{EnvLookupSource, EnvSource, SideFlag};
use super::evidence::{evidence_from_modrinth, put, rank, Evidence, EvidenceMap};
use super::ident::{is_slug, slugs_from_file_name};
use super::jar::JarProbe;

/// 字面量单源在 `core::data_root`（那张表同时是卸载壳的删除清单）
const INDEX_FILE: &str = crate::core::data_root::CACHE_ENV_INDEX;

/// 本地索引里一条结论的保鲜期。端声明基本是静态的，但「作者后来修正了服务端支持」
/// 与「镜像快照滞后」都是真事：过期的结论照常垫底（在线轮被掐时它不至于裸奔），
/// 但仍进反查队列争取刷新——答上了就按 `put` 的等档/更优档规则覆盖
const INDEX_TTL_DAYS: i64 = 90;

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn sha1_key(h: &str) -> String {
    format!("sha1:{}", h.to_lowercase())
}
fn project_key(p: &str) -> String {
    format!("proj:{}", p.to_lowercase())
}
fn fp_key(fp: u32) -> String {
    format!("fp:{fp}")
}

/// 索引条目的落盘形状：证据 + 记录时间。`ts` 缺失 = 端 TTL 上线前写的老条目（按过期处理，
/// 下一轮重查一次并补上时间戳）。`flatten` 进 `Evidence` 的字段 ⇒ 盘上形状对老文件向后兼容
#[derive(Serialize, Deserialize)]
struct StoredEvidence {
    #[serde(flatten)]
    ev: Evidence,
    /// 记录时间（unix 秒）；`None` = 老条目，按过期处理
    #[serde(default)]
    ts: Option<i64>,
}

impl StoredEvidence {
    fn fresh(&self) -> bool {
        self.ts
            .is_some_and(|t| now_secs() - t < INDEX_TTL_DAYS * 24 * 3600)
    }
}

/// `cache_dir/env-index.json`：sha1 / 项目 / 指纹 → 端证据。联网层查到的结果写这里，
/// 下次同一模组（哪怕在另一个包里）离线即答。
///
/// `transparent`：盘上存的就是这张表本身（`save` 写 `&self.map`），不是 `{"map": …}` 的包装。
/// 少了这一句，读回来时外层字段对不上、整表被 serde 当未知键丢掉 ⇒ **写进去的取证永远读不出来**，
/// 每一轮都从零发请求（离线即答形同虚设，也正是「有时自动分类特别久」的一条真凶）
#[derive(Default, Deserialize)]
#[serde(transparent)]
pub struct EnvIndex {
    map: HashMap<String, StoredEvidence>,
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

    fn stored(&self, key: &str) -> Option<&StoredEvidence> {
        self.map.get(key)
    }

    /// 本地索引是否已答过这一份字节：答过就不必为它解 class（命令层的省钱闸门）
    pub fn has_sha1(&self, sha1: &str) -> bool {
        self.map.contains_key(&sha1_key(sha1))
    }

    fn record(&mut self, key: String, ev: Evidence) {
        self.map.insert(key, StoredEvidence { ev, ts: Some(now_secs()) });
    }

    /// 只把 `ts` 刷到现在，值一个字不动。盖章不是重新判定：结论要变只走 `record`
    /// （见 `stamp_stale`：本轮重问过、平台还是那份 ⇒ 盖个时间戳，下一轮别再白问）
    fn touch(&mut self, key: &str) {
        if let Some(s) = self.map.get_mut(key) {
            s.ts = Some(now_secs());
        }
    }
}

/// 一个待反查的模组行
#[derive(Clone)]
pub struct Target {
    /// 包内条目路径（EvidenceMap 的键）
    pub path: String,
    pub sha1: Option<String>,
    /// jar 字节的 CF murmur2 指纹：sha1 在 Modrinth 答不上（CF 独占的 Forge 模组）、
    /// slug 又猜不出时，向 CF 按文件反查的唯一身份钥匙（见 `resolve_via_fingerprints`）
    pub cf_fingerprint: Option<u32>,
    /// 权威项目引用：Modrinth 下载 URL 里的 project_id，查到即采信
    pub project_id: Option<String>,
    /// slug 候选（模组自报 id 在前，文件名切出的 id 在后）；返回项目名字对得上才采信
    pub slugs: Vec<String>,
    /// 可读模组名（按名搜索兜底；优先取 jar 内作者写的显示名）
    pub title: Option<String>,
    /// 字节是否实际在源包里（overrides/mods 索引外文件 / 裸 zip 内 jar 为真）。
    /// 百科腿的行序用它：**包内的行优先查**——它们是真正会让服务端缺件的行，
    /// 未下载的行留到预算尾部（那些即使留待人工，也不影响已有字节的对账）
    pub in_pack: bool,
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
        if t.cf_fingerprint.is_none() {
            t.cf_fingerprint = p.cf_fingerprint;
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

/// 用本地索引填空缺（不发请求）；返回仍需联网的行，以及其中「盘上有结论、只是过期」的那一部分。
/// 命中的行里**过期的结论照常垫底**（在线轮被掐时它不至于裸奔），但仍进反查队列争取刷新：
/// 作者修正过声明、镜像快照滞后的旧结论不会因为索引热就永远压着新的真话。
pub fn apply_index(index: &EnvIndex, targets: &[Target], out: &mut EvidenceMap) -> Pending {
    let mut rows = Vec::new();
    let mut stale = HashMap::new();
    for (i, t) in targets.iter().enumerate() {
        // jar 自证已答上的行不进反查队列：那是最高可信层，再查一遍只会更差
        if out.contains_key(&t.path) {
            continue;
        }
        match locate(index, t) {
            Some((key, s)) => {
                put(out, &t.path, s.ev);
                if !s.fresh() {
                    stale.insert(i, key);
                    rows.push(i);
                }
            }
            None => rows.push(i),
        }
    }
    Pending { rows, stale }
}

/// 索引里这一行的落点：先按哈希（精确到字节），再按项目键（权威 id → slug → 显示名逐个试）。
/// 要把**命中的那个键**一起带出去：盖章只刷 `ts`、不碰值，事后重新推一遍键会和落盘时不是同一个
fn locate<'a>(index: &'a EnvIndex, t: &Target) -> Option<(String, &'a StoredEvidence)> {
    if let Some(h) = t.sha1.as_deref() {
        let key = sha1_key(h);
        if let Some(s) = index.stored(&key) {
            return Some((key, s));
        }
    }
    for p in t
        .project_id
        .iter()
        .chain(t.slugs.iter())
        .chain(t.title.iter())
        .map(|s| s.as_str())
    {
        let key = project_key(p);
        if let Some(s) = index.stored(&key) {
            return Some((key, s));
        }
    }
    None
}

/// `apply_index` 的返回，两份分开是因为修法完全不同：
/// - `rows` = 仍需联网的行下标；
/// - `stale` = 其中「盘上**有**结论、只是 `ts` 空或过了 90 天」的行 → 命中的索引键。
///   这些行本轮重问过、平台还是没有新答案 ⇒ 收尾时给它盖个 `ts`（`stamp_stale`），下轮不再重问；
///   不在这里的那些是「索引压根没这一行」，要省它得靠「问过且没有」的负记录（那一条另议）。
///
/// `stale` 的归属只能在这一层判：命令层那侧若改从「`ev` 里有没有这一行」倒推，就必须卡在
/// CF 端标签播种之前，晚一步会把刚贴上的官方声明数成「过期结论」
pub struct Pending {
    pub rows: Vec<usize>,
    pub stale: HashMap<usize, String>,
}

impl Pending {
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// 逐行项目/搜索反查的请求上限：超大包剩下的行留未判定（方案照出，不为一轮跑上几分钟）。
/// **它挡的不是 429**：2026-09-27 实测 `X-Ratelimit-Limit: 300` 配 1 秒窗口（连发十几发才看得见
/// `Remaining` 从 300 往下掉，隔秒再发就回满），而我们最宽那档 16 路并发，离那条线还差一个数量级。
/// 真正会被顶到的是 `ONLINE_BUDGET` 那 60 秒和「一次进页打几百发」的时间账。
const PROJECT_BUDGET: usize = 150;
/// 搜索腿的**独立**预算。曾经它与 project 腿共用 150（`MAX - project用量`）——
/// project 用得越多 search 分得越少，大包里 project 先把预算吃光，
/// 「按显示名搜索」这条腿一行都没跑过（FarmingTales 包 72 行实测：0 发）
const SEARCH_BUDGET: usize = 150;

/// 第 3 层一批发多少个哈希。接口一次能吃 1000，但元数据请求**不重试**，一发挂了这一批
/// 本轮就全没结论；1000 份构建对象的响应拿 `METADATA_TIMEOUT` 那 10 秒去兜也不算宽裕。
/// 取 500：请求数是旧写法（200）的五分之二，失败半径只到接口顶的一半。
const SHA1_BATCH: usize = 500;

/// 官方档的并发宽度：这些行互不依赖，串行时一个慢请求就拖住整队（整轮慢的主因）。
/// 2026-10-02 实测一枚 mrpack「待查 46 行 · 用时 12976ms」≈ 单请求一秒、4 路消化，抬到 8 路
/// 减半（他实测回到约 7 秒）；再抬到 16 路是同一笔账的再一半，且不撞限流：官方实测
/// `X-Ratelimit-Limit: 300`/秒（见 `MAX_LOOKUP_REQUESTS` 的注释），16 路按 ~0.5 秒一发作是
/// 三十多发每秒，离那条线还差一个数量级。它压不动的只有队首那发哈希批量与串行的百科腿
const OFFICIAL_CONCURRENCY: usize = 16;

/// 麦块档**不跟着抬**，留在 4：对方公布 600 次/分钟，而 `MIRROR_*_BUDGET` 那两条额度当初就是
/// 按「一轮最多 2（自检）+150+150=302 发，留一半余量给同一轮的模组下载」定的。抬到 16 路按 ~1 秒
/// 一发算是 960 次/分钟，直接越过那条公布额度——这一档慢一点换来的是额度可预测，不是白等
const MIRROR_CONCURRENCY: usize = 4;

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

/// 百科补全腿的行上限（每行最多两发：搜索页 + 词条页 ⇒ 最坏 48 个请求）。
/// 排在平台各腿之后，只处理它们**一条都没答上**的行——那种行通常只剩十几行，
/// 这个额度基本吃不满；吃不满是好事，顶到的是设计上限，不是故障。
/// 百科腿的独立墙钟预算。**不进平台腿的 60 秒**：串行一行两发、48 行预算，
/// 实测单发 0.5-1.5s ⇒ 90 秒能跑完大半；到点掐掉时已答的行照常落盘，下一轮接着跑
pub const MCMOD_BUDGET: Duration = Duration::from_secs(90);

/// CF slug 搜索腿的行预算：每行一发（modId 首候选），Modrinth 全落空的行才进这条腿
const CF_SEARCH_ROW_BUDGET: usize = 96;

/// CF slug 搜索腿的独立墙钟：一发/行 × 96 行 × ~0.3s，60 秒足够；
/// 每发另有 METADATA_TIMEOUT 兜底
pub const CF_SEARCH_BUDGET: Duration = Duration::from_secs(60);

const MCMOD_ROW_BUDGET: usize = 96;

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
            // 项目腿没跑成的行**转投搜索腿**（一行一发，按显示名搜）——
            // 曾经这里直接丢弃：这些行连 search 都没享受过，只能落「需人工」。
            // capped 照样报（本轮确实没全跑完），但行不再失踪
            capped = true;
            no_cand.push(i);
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
/// 这里刻意不把「撞顶」当**故障**报（预算上限是设计里的，剩下的行本轮没结论、下轮再查），
/// 但要把这个事实带出去：盖章那条闸门靠它分清「问过了、确实没有」与「压根没派出去」
fn mirror_project_queue(
    targets: &[Target],
    unresolved: &[usize],
    max: usize,
) -> (Vec<(usize, Vec<String>)>, bool) {
    let mut queue = Vec::new();
    let mut used = 0usize;
    let mut cut = false;
    for &i in unresolved {
        let slugs = &targets[i].slugs;
        if slugs.is_empty() {
            continue;
        }
        let room = max.saturating_sub(used);
        if room == 0 {
            cut = true;
            break;
        }
        let take = slugs.len().min(room);
        used += take;
        queue.push((i, slugs.iter().take(take).cloned().collect()));
        if take < slugs.len() {
            cut = true;
            break;
        }
    }
    (queue, cut)
}

/// 「过期重排」的行本轮重问过、平台还是给不出新答案 ⇒ 把盘上那份原结论盖个新 `ts`。
/// `StoredEvidence` 的注释一直写着「下一轮重查一次并补上时间戳」，可 `record` 的调用点
/// 全挂在「拿到证据」那支 ⇒ `ts:null` 的老条目每轮进队、每轮重问、每轮不盖章，
/// 自动分类的时长就长期钉在这批行上。
///
/// **三道闸，缺一条都不盖**（盖章等于「信它一个 TTL」，把「没问到」当成「问了没有」是最贵的误判）：
/// - 调用方只在整轮 `ok` 时进来：到点被掐、请求报错、额度掐顶的行那是「没问到」
/// - `out` 里仍是索引那一份：本轮答上了的行值已经变了（那种行自己也 `record` 过新 `ts`），
///   拿旧值再写一次会把更好的结论落回盘上
/// - 本轮真派了请求出去问过它：官方档带哈希的行由批量腿覆盖（`sha1_asked`），其余行要手里
///   有任何一条身份候选（权威 id / slug / 显示名）才算问到过。三种都没有的行连请求都没发出去，
///   那是「没问」不是「问了没有」
fn stamp_stale(
    index: &mut EnvIndex,
    targets: &[Target],
    pending: &Pending,
    out: &EvidenceMap,
    sha1_asked: bool,
) {
    for (&i, key) in &pending.stale {
        let t = &targets[i];
        let asked = (sha1_asked && t.sha1.is_some())
            || t.project_id.is_some()
            || !t.slugs.is_empty()
            || t.title.is_some();
        if !asked {
            continue;
        }
        // 本轮答上过的行已经带着新 ts `record` 过了，与盖章无关
        let Some(stored) = index.stored(key) else {
            continue;
        };
        if stored.fresh() {
            continue;
        }
        // `out` 里仍是索引那一份 ⇒ 「重问过、还是它」。值一不同就说明本轮拿到了别的结论，
        // 那份归它自己的 `record` 落盘，别拿旧值盖回去
        if out.get(&t.path) != Some(&stored.ev) {
            continue;
        }
        index.touch(key);
    }
}

/// 在线反查：**设置里选了哪个源就只问那个源**，不再「先镜像、答不上回落官方」——
/// 那样两家的钱都付一遍，选了国内源反而比纯官方更慢（镜像 0.2s + 官方 2.2s 串在一行上）。
/// `Minekuai` ⇒ 平台那条链只发 `api.minekuai.cn`（含 sha1 批量那条 Modrinth-only 接口也不发，
/// 见 `resolve_via_mirror` 的代价说明）；`Official` ⇒ 官方三条腿照旧，镜像连存活自查都不做。
/// `Off` 不进这里：调用方（`resolve_local_jar` 与 `classify_pack`）在派轮之前就用 `is_off()`
/// 拦掉了，这里真收到 `Off` 也只按官方那一档走。
/// 两档末尾都挂着同一条收尾（`shared_tail`）：CF 指纹腿（官方平台、凭用户自己的 Key，
/// 与「端信息反查源」那档设置无关——cfpack 的 CF 补取同口径）与百科补全腿（看 `mcmod` 开关）。
/// 结果同时写回 out 与索引，并且分批落盘。
/// 返回是否跑完整轮：false = 有请求失败或超出额度上限（前端提示「联网反查未全部完成」）
pub async fn resolve_online(
    dl: &Downloader,
    index: &mut EnvIndex,
    cache_dir: &Path,
    targets: &[Target],
    pending: &Pending,
    out: &mut EvidenceMap,
    source: EnvLookupSource,
) -> bool {
    if source.is_minekuai() {
        return resolve_via_mirror(dl, index, cache_dir, targets, pending, out).await;
    }
    let mut ok = true;

    // 第 3 层：按 sha1 批量反查构建（接口上限 1000 个哈希，分批见 `SHA1_BATCH`）
    let hashes: Vec<String> = pending
        .rows
        .iter()
        .filter_map(|i| targets[*i].sha1.clone())
        .map(|h| h.to_lowercase())
        .collect();
    let mut hits: HashMap<String, ModrinthEnv> = HashMap::new();
    for chunk in hashes.chunks(SHA1_BATCH) {
        match dl.version_env_by_sha1(chunk).await {
            Ok(m) => hits.extend(m),
            Err(_) => ok = false,
        }
    }
    let mut unresolved = Vec::new();
    for i in &pending.rows {
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
    let (queue, mut by_name, _requests, capped) =
        project_queue(targets, &unresolved, PROJECT_BUDGET);
    if capped {
        ok = false;
    }
    // 一行的候选链**链内串行**（命中即 break）
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
                .buffer_unordered(OFFICIAL_CONCURRENCY)
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
    let run = queries.len().min(SEARCH_BUDGET);
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
                .buffer_unordered(OFFICIAL_CONCURRENCY)
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

    // 收尾：CF 指纹腿（官方平台补全）+ 百科补全腿（看设置开关）。均为补全，失败不改 `ok`
    shared_tail(dl, index, cache_dir, targets, pending, out).await;

    // 过期重排的行本轮**成功**重问过还是没有新答案 ⇒ 盖章，下一轮不再重问（`ok=false` 时
    // 一行都不盖：那意味着有请求报错或额度掐顶，剩下的「没答案」是「没问到」，不能当结论存）
    if ok {
        stamp_stale(index, targets, pending, out, true);
    }
    index.save(cache_dir);
    ok
}

/// 麦块档的整条平台链：**只发 `api.minekuai.cn`**，官方三条腿（sha1 批量 / 项目 / 搜索）一条都不碰
/// （末尾那条百科补全腿除外，它不是平台源）。
///
/// 两条入口各发一发自检后才派腿（路径被改名时它回的是 200 + `{"code":404}`，只看状态码会被骗）。
/// 这一档里镜像就是**用户选中的那个源**，不是加速件 ⇒ 自检验不过就等于这一轮没跑完（`ok=false`，
/// 前端如实提示「联网反查未全部完成」），不再像旧口径那样悄悄回落官方。
///
/// **这一档拿不到的东西**（设置项的说明文案要对得上）：
/// - 第 3 层「按 sha1 批量反查构建」（精确到这一个文件的 `environment`，rank 1）：它没有对应端点；
/// - 权威 `project_id` 那条候选（URL 里带出的那个）：它只认 slug，`detail/u6dRKJwZ` 实测 404；
/// - 不在收录的模组（抽样的 `coppered-equipment` 就 404）：Modrinth 侧没有第二个源可问，
///   只剩末尾那条百科补全腿（`resolve_via_mcmod`，问的是 `mcmod.cn`）；它也没答上才停在「未判定」。
///
/// 落盘的结论一律标 `MirrorProject`（rank 3），阶梯与官方档同一条：已经在索引里的 jar/hash 级证据
/// 不会因为换档被压掉（`put` 只认等档或更好档）。
async fn resolve_via_mirror(
    dl: &Downloader,
    index: &mut EnvIndex,
    cache_dir: &Path,
    targets: &[Target],
    pending: &Pending,
    out: &mut EvidenceMap,
) -> bool {
    // 存活自查：两条入口各一发（这一档没有官方链兜着，验不过就等于这一轮没跑完）
    let health = dl.mirror_health().await;
    let mut ok = health.project && health.search;
    // 镜像两条腿的额度掐顶**不报 ok**（那是设计上限，不是故障）⇒ 盖章要单独知道它发生过
    let mut capped = false;
    let mut answered: HashSet<usize> = HashSet::new();

    // 项目腿：候选全是猜的（镜像没有权威 id 那条路），所以一律要名字对得上才算采信
    if health.project {
        let (queue, cut) = mirror_project_queue(targets, &pending.rows, MIRROR_PROJECT_BUDGET);
        capped |= cut;
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
                    .buffer_unordered(MIRROR_CONCURRENCY)
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
                .rows
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
        capped |= run < queries.len();
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
                    .buffer_unordered(MIRROR_CONCURRENCY)
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

    // 收尾：CF 指纹腿（官方平台补全）+ 百科补全腿（看设置开关）。CF 是独立平台，
    // 指纹腿与「选了麦块就只问麦块」的口径不冲突——破的只是「Modrinth 官方一条不发」那句
    shared_tail(dl, index, cache_dir, targets, pending, out).await;

    // 盖章的闸门比官方档多一条：这一档两条腿撞顶是**静默**的（设计上限，不报 ok=false），
    // 所以撞过顶就连环都不盖——没派出去的行不能按「问过了没有」存进索引。
    // `sha1_asked=false`：镜像没有「按哈希批量反查构建」那条端点，光有哈希的行在这档没被问过
    if ok && !capped {
        stamp_stale(index, targets, pending, out, false);
    }

    index.save(cache_dir);
    ok
}

/// 两条平台档（官方 / 镜像）共用的收尾：CF 指纹腿 + 百科补全腿。
/// 都是补全——请求失败不改调用方的 `ok`；百科腿还要看设置开关（默认关，
/// 它是社区二手声明 + HTML 解析，见 `resolve_via_mcmod` 的纪律说明）。
async fn shared_tail(
    dl: &Downloader,
    index: &mut EnvIndex,
    cache_dir: &Path,
    targets: &[Target],
    pending: &Pending,
    out: &mut EvidenceMap,
) {
    resolve_via_fingerprints(dl, index, cache_dir, targets, pending, out).await;
}

/// CF 指纹腿一批发多少个指纹。接口单批上限 128，取 100 留余量；
/// 真正的花销是「每 100 行一发请求」，比项目腿的一行一发低两个量级
const FP_BATCH: usize = 100;

/// CF 指纹腿：sha1 / 项目 / 搜索各层都没答上、但手里有 murmur2 指纹的行（CF 独占的
/// Forge 模组就长这样——不在 Modrinth 上，哈希反查必然落空），向 CF 按文件反查。
/// 匹配上的行从 file 对象取构建级端标签（`EnvSource::CfFile`）与 sha1——证据同时挂
/// `fp:` 与 `sha1:` 两个键，下次离线即答。
///
/// 它排在这里而不是平台腿之前：平台腿答上的行（Modrinth 哈希/项目）比 CF 标签更优或同级，
/// 先跑可以省下指纹腿的行；反过来指纹腿答上的行，是平台各层确实无解的那批。
/// 现有证据**不优于** CF 构建标签的行才进场（镜像 / 百科 / mrpack / 名称层都算）——
/// jar 自证与 Modrinth 哈希/项目答过的行再问一轮只会得到更差的结论，白花请求。
///
/// **失败不改调用方的 `ok`**：CF 是独立平台，这一腿没答上不该把 Modrinth 那侧
/// 演成「联网反查未全部完成」；没答上的行留在待查列，下一轮再试
async fn resolve_via_fingerprints(
    dl: &Downloader,
    index: &mut EnvIndex,
    cache_dir: &Path,
    targets: &[Target],
    pending: &Pending,
    out: &mut EvidenceMap,
) {
    let rows: Vec<usize> = pending
        .rows
        .iter()
        .copied()
        .filter(|&i| {
            targets[i].cf_fingerprint.is_some()
                && out
                    .get(&targets[i].path)
                    .is_none_or(|ev| rank(ev.source) >= rank(EnvSource::CfFile))
        })
        .collect();
    // 指纹去重：同一份 jar 在包里出现两次（或两行指向同一构建）只问一次
    let mut uniq: Vec<u32> = rows
        .iter()
        .filter_map(|&i| targets[i].cf_fingerprint)
        .collect();
    uniq.sort_unstable();
    uniq.dedup();
    // 指纹只可能出自「清单没给哈希、包内扫描现算字节」那批行：`files[]` 自带哈希的行不重算字节
    // ⇒ 纯 mrpack 包这腿 0 发；`overrides/` 里没进清单的 jar、裸 .zip 与手动塞入的 jar 才有
    // （CF 来源的包靠 cfpack 那一趟贴端标签）
    if uniq.is_empty() {
        return;
    }
    for chunk in uniq.chunks(FP_BATCH) {
        let Ok(matches) = dl.curseforge_fingerprints(chunk).await else {
            return; // 缺 Key / 网络故障：本轮收队，已拿到的部分照常落盘
        };
        for &i in &rows {
            let t = &targets[i];
            let Some(fp) = t.cf_fingerprint else { continue };
            let Some(hit) = matches.get(&fp) else { continue };
            let Some((c, s)) = hit.sides else { continue };
            let ev = Evidence {
                client: Some(c),
                server: Some(s),
                source: EnvSource::CfFile,
            };
            put(out, &t.path, ev);
            index.record(fp_key(fp), ev);
            if let Some(h) = &hit.sha1 {
                index.record(sha1_key(h), ev);
            }
        }
        index.save(cache_dir);
    }
}

/// 补全腿：平台各腿（官方三条 / 镜像两条）**全答不上**的那些行，才去 MC百科各问一次。
/// 只在 `shared_tail` 里调用（官方档与镜像档共用），所以对已有证据的行零影响：
/// 一行只要被 jar / 哈希 / 项目 / 镜像任何一层答过，`out` 里就有它的键，这一层连请求都不发。
///
/// **它的失败不改 `complete`**（返回 `()`，不进 `ok`）：这一层是补全，不是用户选中的那个源。
/// 百科挂了或被拦了，平台那一轮照样算跑完；把「补充源没问到」演成「联网反查未全部完成」，
/// 只会让人去点重新分类，而重跑一遍改不了任何结论。
///
/// 四种「问不到」分开走，混起来就是把「没有」记成「有」：
/// - 搜不到词条 / 词条没写运行环境 / 只写了一端 ⇒ 没有依据，什么都不落；
/// - 名字对不上 ⇒ 不采信（同名衍生分支与别的模组就在这一条上被挡掉）；
/// - 200 + 那段跳首页的人机验证脚本 ⇒ 整条腿当场收队，剩下的行本轮不问（继续敲只会加深拦截）；
/// - 请求本身失败 ⇒ 那一行本轮没结论，下一轮再问。
///
/// 采信要**两道同形**：搜索列表里那条的名字要对得上，词条页标题也得对得上（页面改版时
/// 列表与详情页不会同时恰好糊成一个对得上的名字）。结论同时挂在这一行的 sha1 与全部项目候选下，
/// 下次离线即答——与其余各层同一口径，也同样受「索引保鲜期」管着（见 `INDEX_TTL_DAYS`）。
/// CF slug 搜索反查腿：Modrinth 全落空的行，用 jar 内 modId / 文件名 slug
/// 向 CF 按项目搜索（`curseforge_slug_sides`）——CF 收录远大于 Modrinth，且
/// CF 的 slug 与 jar 内 modId 高度一致（实测 `biomesize` 一击命中）。命中项目
/// 后聚合项目级端标签（`EnvSource::CfFile`），结论挂 `proj:` 键（CF slug）与
/// `sha1:` 键，下次离线即答。
///
/// 行序：**包内的行排前**（与百科腿同一理由：它们才是会让服务端缺件的行）。
/// 行预算见 `CF_SEARCH_ROW_BUDGET`，每行最多发一发（取首个 slug 候选 = modId，
/// 它与 CF slug 同形的概率最高）；404 / 同形失败不计故障
pub(crate) async fn resolve_via_cf_search(
    dl: &Downloader,
    index: &mut EnvIndex,
    cache_dir: &Path,
    targets: &[Target],
    out: &mut EvidenceMap,
) {
    let mut rows: Vec<(usize, bool, String)> = targets
        .iter()
        .enumerate()
        .filter(|(_, t)| !out.contains_key(&t.path))
        .filter_map(|(i, t)| {
            let slug = t.slugs.first()?.trim().to_string();
            (!slug.is_empty()).then_some((i, t.in_pack, slug))
        })
        .collect();
    rows.sort_by_key(|(_, in_pack, _)| !*in_pack);
    for (i, _, slug) in rows.into_iter().take(CF_SEARCH_ROW_BUDGET) {
        let t = &targets[i];
        match dl.curseforge_slug_sides(&slug).await {
            Ok(Some((c, srv))) => {
                let ev = Evidence {
                    client: Some(c),
                    server: Some(srv),
                    source: EnvSource::CfFile,
                };
                put(out, &t.path, ev);
                index.record(project_key(&slug), ev);
                if let Some(h) = &t.sha1 {
                    index.record(sha1_key(h), ev);
                }
                index.save(cache_dir);
            }
            // 404 / 同形失败 / 没勾标签：本行无结论，下一层（百科）接着
            Ok(None) => {}
            Err(_) => {}
        }
    }
}

pub(crate) async fn resolve_via_mcmod(
    dl: &Downloader,
    index: &mut EnvIndex,
    cache_dir: &Path,
    targets: &[Target],
    out: &mut EvidenceMap,
) {
    // 行序：**包内的行排前**（字节实际在 overrides/mods 的那些——它们才是会让服务端
    // 缺件的行；未下载的行即使这一轮没查到，也不影响已有字节的对账）。同一优先级内
    // 保持 targets 原序（稳定排序，不搅动既有预算分配的可预期性）
    let mut rows: Vec<(usize, bool, String)> = targets
        .iter()
        .enumerate()
        .filter(|(_, t)| !out.contains_key(&t.path))
        .filter_map(|(i, t)| {
            let q = t
                .title
                .as_deref()
                .or(t.slugs.first().map(|s| s.as_str()))
                .map(str::trim)
                .filter(|q| !q.is_empty())?;
            Some((i, t.in_pack, q.to_string()))
        })
        .collect();
    rows.sort_by_key(|(_, in_pack, _)| !*in_pack);
    let rows: Vec<(usize, String)> =
        rows.into_iter().map(|(i, _, q)| (i, q)).collect();
    // 一行两发、**整条腿串行**：这家对并发极不友好（实测两路并发时同一句查询会间歇性回
    // 22KB 的空结果页，串行 8 轮 16 发全数正常），而空结果页与「真查无此模组」长得一模一样，
    // 补问一轮也救不回来（实测仍会抖）。慢一点换的是结论稳定，值得
    for (i, query) in &rows[..rows.len().min(MCMOD_ROW_BUDGET)] {
        let (got, blocked) = mcmod_ask(dl, &targets[*i], query).await;
        if let Some((keys, ev)) = got {
            store_mcmod(index, out, targets, *i, &keys, ev);
            index.save(cache_dir);
        }
        if blocked {
            break;
        }
    }
}

/// 一行的百科问答：搜索页找名字对得上的词条，再进词条页取「运行环境」。
/// 返回 `(结论, 被拦)`；`被拦` 只有那一段跳首页的人机验证脚本会置，
/// 作用是让上面那条腿当场收队。
///
/// **两条反直觉的口径，改之前先读**：
/// - 别给这条链挂浏览器 User-Agent：实测同一个 `key=sodium`，我们的默认 UA 回带 30 条结果的页面，
///   而 Chrome UA 回 20KB 的空结果页（两边都是 200）。换 UA 不会绕过拦截，只会把每一行都变成「查无」；
/// - 词条页缺 `<title>`（解析不出）时放过第二道闸：列表那条已经对上了，详情页没标题只是改版，
///   不构成「这是另一个模组」的证据
async fn mcmod_ask(
    dl: &Downloader,
    t: &Target,
    query: &str,
) -> (Option<(Vec<String>, Evidence)>, bool) {
    // 比对用的候选：显示名在前、文件名切出的 slug 在后（与官方那条同一份名单）
    let cands: Vec<String> = t.title.iter().chain(t.slugs.iter()).cloned().collect();
    let hits = match dl.mcmod_search(query).await {
        Ok(McmodPage::Answered(h)) => h,
        Ok(McmodPage::Blocked) => return (None, true),
        // 空结果页与「真查无此模组」同形，本轮就当没有依据：补问救不回来（见上面那条腿）
        Ok(McmodPage::Absent) => return (None, false),
        Err(_) => return (None, false),
    };
    let Some(hit) = hits.iter().find(|h| mcmod_confident(&cands, &h.name)) else {
        eprintln!(
            "[mcmod] 名字闸拒绝：候选 {cands:?} ↔ 词条 {names:?}（共 {count} 条）",
            names = hits.iter().map(|h| h.name.as_str()).collect::<Vec<_>>(),
            count = hits.len(),
        );
        return (None, false);
    };
    let entry = match dl.mcmod_entry(&hit.id).await {
        Ok(McmodPage::Answered(e)) => e,
        other => {
            eprintln!("[mcmod] 词条页未答上：{} → {:?}", hit.id, other.as_ref().map(|_| ()));
            return (None, false);
        }
    };
    // 第二道同形闸
    if !entry.name.is_empty() && !mcmod_confident(&cands, &entry.name) {
        return (None, false);
    }
    let (Some(c), Some(s)) = (entry.client, entry.server) else {
        return (None, false);
    };
    (
        Some((
            cands,
            Evidence {
                client: Some(c),
                server: Some(s),
                source: EnvSource::Mcmod,
            },
        )),
        false,
    )
}

/// 百科结论落进证据表与本地索引：这一行的 sha1 与全部项目候选都挂同一条结论，下次离线即答
fn store_mcmod(
    index: &mut EnvIndex,
    out: &mut EvidenceMap,
    targets: &[Target],
    i: usize,
    keys: &[String],
    ev: Evidence,
) {
    let t = &targets[i];
    put(out, &t.path, ev);
    if let Some(h) = &t.sha1 {
        index.record(sha1_key(h), ev);
    }
    for k in keys {
        index.record(project_key(k), ev);
    }
}

/// 单个本地 jar 的端取证阶梯（用户手动添加的行走这条路），口径与整包分类完全一致：
/// jar 自证 → 本地索引（离线即答）→ 联网按 `source` 那一档选定的**唯一一个源**反查并落盘
/// （见 `resolve_online`；`source=Off` 时这一层整个跳过）。
/// `cf` = 这枚 jar 的 CF 构建级端标签（版本列表带来的官方声明，本地添加恒 None）：
/// 在 jar 自证与索引结论**之后**播种，`put` 只认等档或更好，真正更优的旧结论压不过它。
/// 返回 `None` = 各层都没结论（前端标「需人工确认」，绝不猜）
pub async fn resolve_local_jar(
    dl: &Downloader,
    cache_dir: &Path,
    file_name: &str,
    probe: &JarProbe,
    cf: Option<(SideFlag, SideFlag)>,
    source: EnvLookupSource,
    mcmod: bool,
) -> Option<Evidence> {
    let key = file_name.to_string();
    let mut out: EvidenceMap = HashMap::new();
    if let Some(ev) = probe.env {
        put(&mut out, &key, ev);
    }
    let mut targets = vec![Target {
        path: key.clone(),
        sha1: probe.sha1.clone(),
        cf_fingerprint: probe.cf_fingerprint,
        in_pack: true, // 单个 jar 必然是用户手里实际存在的文件
        project_id: None,
        slugs: slugs_from_file_name(file_name),
        title: None,
    }];
    let mut probes = HashMap::new();
    probes.insert(key.clone(), probe.clone());
    apply_probes(&probes, &mut targets);
    let mut index = EnvIndex::load(cache_dir);
    let pending = apply_index(&index, &targets, &mut out);
    if let Some((c, s)) = cf {
        put(
            &mut out,
            &key,
            Evidence {
                client: Some(c),
                server: Some(s),
                source: EnvSource::CfFile,
            },
        );
    }
    if !source.is_off() && !pending.is_empty() {
        let _ = resolve_online(
            dl,
            &mut index,
            cache_dir,
            &targets,
            &pending,
            &mut out,
            source,
        )
        .await;
    }
    // 单 jar 同样走 CF slug 搜索与百科腿：用户手里实际存在的文件（in_pack=true 的
    // 语义完全吻合），Modrinth 未收录时 CF slug（与 modId 同形率高）与百科是仅有的数据源
    if !source.is_off() && out.get(&key).is_none() {
        let _ = tokio::time::timeout(
            CF_SEARCH_BUDGET,
            resolve_via_cf_search(dl, &mut index, cache_dir, &targets, &mut out),
        )
        .await;
    }
    if mcmod && !source.is_off() && out.get(&key).is_none() {
        let _ = tokio::time::timeout(
            MCMOD_BUDGET,
            resolve_via_mcmod(dl, &mut index, cache_dir, &targets, &mut out),
        )
        .await;
    }
    out.get(&key).copied()
}

/// 「从网络添加」的构建行走同一条阶梯。CurseForge 的响应里没有结构化的端声明，但它给了
/// **构建字节的 sha1** 与构建级端标签（`gameVersions` 的 Client/Server，前端版本列表解析好的
/// 那两个值从 `cf` 传进来）——sha1 能问到 Modrinth 那两层（按哈希反查构建 → 项目/显示名），
/// 端标签在 Modrinth 答不上时兜底（CF 独占模组），结论与包内行同口径、同样落盘，第二次添加零请求。
/// 与 `resolve_local_jar` 的差别只有 probe 是拼出来的：没有 jar 字节可解，
/// 所以 `env`（作者自证）、`code`（字节码提示）与指纹恒空，只剩平台那几层可走。
/// `sha1` 只收 40 位 hex：拿到脏值就当没有，免得往索引里写一个永远对不上的键。
pub async fn resolve_added_build(
    dl: &Downloader,
    cache_dir: &Path,
    file_name: &str,
    sha1: Option<&str>,
    title: Option<&str>,
    cf: Option<(SideFlag, SideFlag)>,
    source: EnvLookupSource,
    mcmod: bool,
) -> Option<Evidence> {
    let probe = JarProbe {
        sha1: sha1
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| s.len() == 40 && s.chars().all(|c| c.is_ascii_hexdigit())),
        title: title.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
        ..Default::default()
    };
    resolve_local_jar(dl, cache_dir, file_name, &probe, cf, source, mcmod).await
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
                cf_fingerprint: None,
            project_id: None,
            slugs: vec![],
            title: Some("3D Skin Layers".into()),
         in_pack: false,};
        assert!(pick_search_hit("3D Skin Layers", &t, &[other.clone(), hit.clone()]).is_some());
        let t2 = Target { title: Some("No Such Mod".into()), ..t.clone() };
        assert!(pick_search_hit("No Such Mod", &t2, &[other, hit]).is_none());
    }

    fn row(slugs: &[&str]) -> Target {
        Target {
            path: "mods/x.jar".into(),
            sha1: None,
                cf_fingerprint: None,
            project_id: None,
            slugs: slugs.iter().map(|s| s.to_string()).collect(),
            title: None,
         in_pack: false,}
    }

    #[test]
    fn lookup_budget_never_overshoots_and_says_so() {
        // 50 行 × 2 个候选 = 100 次请求，预算只给 5：预留按整条链，project 腿不超发
        let targets: Vec<Target> = (0..50)
            .map(|n| row(&[&format!("m{n}a"), &format!("m{n}b")]))
            .collect();
        let all: Vec<usize> = (0..targets.len()).collect();
        let (queue, no_cand, used, capped) = project_queue(&targets, &all, 5);
        assert_eq!(used, 5, "预留数不能超发：超了就是这一轮的额度形同不存在");
        assert!(capped, "还剩 47 行没查，必须报未全部完成");
        // 前 2 行整链进队，第 3 行只分到 1 个候选，后面的行 project 一条没发——
        // 但它们**转投搜索腿**（no_cand 就是 search 的初始行集），不再像旧实现那样
        // 既不进 project 也不进 search、整行失踪落「需人工」
        assert_eq!(queue[0].1.len(), 2);
        assert_eq!(queue[1].1.len(), 2);
        assert_eq!(queue[2].1, vec![("m2a".to_string(), false)]);
        assert_eq!(queue.len(), 3);
        assert_eq!(no_cand.len(), 47, "超预算的 47 行全部转投搜索腿");
        // 转投的行里有候选（本例 row() 都有 slug），no_cand 命名沿用但语义已扩为
        // 「不进 project 腿的行」
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
    fn mirror_queue_only_takes_slug_candidates_and_reports_truncation() {
        // 镜像只认 slug：project_id 那条权威候选压根不在这一步的输入里（Target 也没带）
        let targets = vec![row(&["a", "b"]), row(&[]), row(&["c"])];
        let all: Vec<usize> = vec![0, 1, 2];
        let (queue, cut) = mirror_project_queue(&targets, &all, 150);
        assert_eq!(
            queue,
            vec![
                (0usize, vec!["a".to_string(), "b".to_string()]),
                (2usize, vec!["c".to_string()])
            ],
            "没候选的行跳过就行：它照旧走官方，不算镜像的失败"
        );
        assert!(!cut, "跳过没候选的行不算撞顶：那是这一行压根没得问");
        // 预算只够第 0 行的第一个候选 ⇒ 该行只派一发，后面的行本轮一条不发
        let (tight, cut) = mirror_project_queue(&targets, &all, 1);
        assert_eq!(tight, vec![(0usize, vec!["a".to_string()])]);
        assert!(cut, "候选链被额度截断、后面的行一条没派 ⇒ 必须报出去（盖章那条闸门吃它）");
    }

    #[tokio::test]
    #[ignore = "真联网：`Minekuai` 档只发镜像两条腿，答不上的行留空（不回落官方）"]
    async fn mirror_round_answers_labels_itself_and_404_is_not_a_failure() {
        // 这一条测的是「加速件」的全部契约，少一条都不算通：
        // ① 存活自查过后，在册的行由镜像答上（来源标 MirrorProject，不冒充平台项目）
        // ② 选了麦块就一条官方请求都不发（含它没有对应端点的 sha1 批量那条），否则这一档白加
        // ③ 镜像查不到的行留空，且 `complete` 仍然是 true——404 不是故障
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
                cf_fingerprint: None,
                project_id: None,
                slugs: vec![slug.to_string()],
                title: None,
             in_pack: false,})
            .collect();
        let all: Vec<usize> = vec![0, 1, 2];
        let mut index = EnvIndex::load(&dir);
        let mut out = EvidenceMap::new();
        let dl = Downloader::new(dir.clone(), 4);
        let pending = Pending {
            rows: all,
            stale: HashMap::new(),
        };
        let ok = resolve_online(
            &dl,
            &mut index,
            &dir,
            &targets,
            &pending,
            &mut out,
            EnvLookupSource::Minekuai,
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


    /// 百科补全腿的真联网用例（本轮起因就是这枚：FTB 任务在两家平台都没端声明）。
    /// 三条判据：① 平台没答上的行由百科答上、来源标 `Mcmod`；② 词条没收录的行留空；
    /// ③ 已有证据的行不该被这条腿改写（`resolve_via_mcmod` 压根不为它发请求）
    #[tokio::test]
    #[ignore = "真联网：百科搜索页 + 词条页各发一发"]
    async fn mcmod_leg_answers_rows_the_platform_left_empty() {
        let dir = std::env::temp_dir().join(format!("sideshift-env-mcmod-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let _ = std::fs::remove_file(dir.join(INDEX_FILE));
        let targets = vec![
            Target {
                path: "mods/ftb-quests-forge.jar".into(),
                sha1: None,
                cf_fingerprint: None,
                project_id: None,
                slugs: vec!["ftbquests".into()],
                title: Some("FTB Quests".into()),
             in_pack: false,},
            Target {
                path: "mods/zz-not-a-real-mod.jar".into(),
                sha1: None,
                cf_fingerprint: None,
                project_id: None,
                slugs: vec!["zz-not-a-real-mod".into()],
                title: None,
             in_pack: false,},
            Target {
                path: "mods/sodium.jar".into(),
                sha1: None,
                cf_fingerprint: None,
                project_id: None,
                slugs: vec!["sodium".into()],
                title: Some("Sodium".into()),
             in_pack: false,},
        ];
        let mut out: EvidenceMap = [(
            "mods/sodium.jar".to_string(),
            Evidence {
                client: Some(SideFlag::Required),
                server: None,
                source: EnvSource::JarMetadata,
            },
        )]
        .into_iter()
        .collect();
        let mut index = EnvIndex::load(&dir);
        let dl = Downloader::new(dir.clone(), 4);
        resolve_via_mcmod(&dl, &mut index, &dir, &targets, &mut out).await;

        assert_eq!(
            out.get("mods/ftb-quests-forge.jar").map(|e| e.source),
            Some(EnvSource::Mcmod),
            "词条在册却没答上 ⇒ 名字闸太严或解析没跟上页面"
        );
        assert_eq!(
            out.get("mods/ftb-quests-forge.jar")
                .and_then(|e| e.server),
            Some(SideFlag::Required),
            "实测词条 1423 写的是「客户端需装, 服务端需装」"
        );
        assert!(
            !out.contains_key("mods/zz-not-a-real-mod.jar"),
            "查无此词条必须留空，绝不拿相近名字凑一条依据"
        );
        assert_eq!(
            out.get("mods/sodium.jar").map(|e| e.source),
            Some(EnvSource::JarMetadata),
            "jar 自证的行不该被补全腿压掉"
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
            None,
            EnvLookupSource::Off,
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

    /// 过期的索引结论照常垫底（在线轮被掐时它不至于裸奔），但仍进反查队列争取刷新；
    /// `record` 落下的新鲜结论照旧离队。`ts: None` / 过老的时间戳都按过期处理
    #[test]
    fn stale_index_hits_fall_back_but_stay_pending() {
        let dir = std::env::temp_dir().join(format!(
            "sideshift-index-stale-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let ev = evidence_from_modrinth(
            &env_of(SideFlag::Required, SideFlag::Unsupported),
            EnvSource::ModrinthHash,
        )
        .expect("required/unsupported 有区分度");
        let key = sha1_key("bb0cb397083a0be2601bd4c6f7060326a1c8505f");
        let mut index = EnvIndex::default();
        // ts = 0（1970）：必过期
        index.map.insert(key.clone(), StoredEvidence { ev, ts: Some(0) });
        index.save(&dir);
        let index = EnvIndex::load(&dir);
        let targets = vec![Target {
            path: "mods/x.jar".into(),
            sha1: Some("bb0cb397083a0be2601bd4c6f7060326a1c8505f".into()),
            cf_fingerprint: None,
            project_id: None,
            slugs: vec![],
            title: None,
         in_pack: false,}];
        let mut out = EvidenceMap::new();
        let pending = apply_index(&index, &targets, &mut out);
        assert!(out.contains_key("mods/x.jar"), "过期结论照常垫底，在线轮被掐也不裸奔");
        assert_eq!(pending.rows, vec![0], "过期的仍进反查队列争取刷新");
        assert!(pending.stale.contains_key(&0), "它同时得是可盖章的那一类");
        // record 刚写的（ts = now）才是新鲜结论：照旧离队，也没有可盖的键
        let mut fresh = EnvIndex::default();
        fresh.record(key, ev);
        let mut out2 = EvidenceMap::new();
        let pending2 = apply_index(&fresh, &targets, &mut out2);
        assert!(pending2.is_empty(), "新鲜结论不该重查");
        assert!(pending2.stale.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 盖章那条闸门（`stamp_stale`）：过期行本轮真被问过、平台还是给不出新答案 ⇒ 只刷 `ts`、
    /// 值原样不动，下一轮它就不进反查队列了。三条反例各挡一种「其实没问到」——
    /// 手里没身份的行（连请求都没发出去）、值已被更好结论改写的行（拿旧值盖回去＝倒退）、
    /// 以及镜像档那侧「只有哈希、没有那条批量端点」的行
    #[test]
    fn stale_rows_are_stamped_only_when_the_reask_was_clean() {
        let h = |c: char| c.to_string().repeat(40);
        let ev = evidence_from_modrinth(
            &env_of(SideFlag::Required, SideFlag::Unsupported),
            EnvSource::ModrinthHash,
        )
        .expect("required/unsupported 有区分度");
        let mut index = EnvIndex::default();
        // ts = 0（1970）：三行全部按过期处理
        for c in ['a', 'b', 'c'] {
            index
                .map
                .insert(sha1_key(&h(c)), StoredEvidence { ev, ts: Some(0) });
        }
        let target = |path: &str, c: char, slug: Option<&str>| Target {
            path: path.into(),
            sha1: Some(h(c)),
            cf_fingerprint: None,
            project_id: None,
            slugs: slug.into_iter().map(String::from).collect(),
            title: None,
         in_pack: false,};
        let targets = vec![
            target("mods/asked.jar", 'a', Some("asked")),
            target("mods/nameless.jar", 'b', None),
            target("mods/answered.jar", 'c', Some("answered")),
        ];
        let mut out = EvidenceMap::new();
        let pending = apply_index(&index, &targets, &mut out);
        assert_eq!(pending.stale.len(), 3, "三行都是「盘上有结论、只是过期」");
        // 第三行本轮被更高的层答上了（jar 自证）：`out` 里已经不是索引那一份
        out.insert(
            "mods/answered.jar".to_string(),
            Evidence {
                client: Some(SideFlag::Required),
                server: Some(SideFlag::Required),
                source: EnvSource::JarMetadata,
            },
        );

        stamp_stale(&mut index, &targets, &pending, &out, false);
        assert!(
            index.stored(&sha1_key(&h('a'))).unwrap().fresh(),
            "有 slug 候选 ⇒ 本轮派过请求 ⇒ 该盖"
        );
        assert!(
            !index.stored(&sha1_key(&h('b'))).unwrap().fresh(),
            "没身份的行连请求都没发出去，那是「没问」不是「问了没有」"
        );
        assert!(
            !index.stored(&sha1_key(&h('c'))).unwrap().fresh(),
            "值已变 ⇒ 新结论归它自己的 record 落盘，别拿旧值盖回去"
        );
        // 同一行在官方档就该盖章：sha1 批量那条腿确实问了它
        stamp_stale(&mut index, &targets, &pending, &out, true);
        assert!(
            index.stored(&sha1_key(&h('b'))).unwrap().fresh(),
            "官方档有按哈希批量反查那条腿 ⇒ 算问到过"
        );
        assert_eq!(
            index.stored(&sha1_key(&h('a'))).unwrap().ev,
            ev,
            "盖章只刷时间戳，结论一个字都不动"
        );
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
            back.stored(&key).map(|s| (s.ev.source, s.ev.client, s.ev.server)),
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
            None,
            EnvLookupSource::Off,
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
                None,
                EnvLookupSource::Off,
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
                cf_fingerprint: None,
            project_id: None,
            slugs: vec!["x".into()],
            title: None,
         in_pack: false,}];
        apply_probes(&probes, &mut targets);
        assert_eq!(targets[0].sha1.as_deref(), Some("ABCdef"));
        assert_eq!(targets[0].title.as_deref(), Some("3D Skin Layers"));
        // 模组自报 id 排在文件名猜测之前，且小写归一
        assert_eq!(targets[0].slugs, vec!["skinlayers3d", "x"]);
    }
}
