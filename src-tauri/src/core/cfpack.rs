//! CurseForge 官方导出包的「按编号补取」层：`files[]` 只给 `{projectID, fileID}`，联网换回「名字 + 大小 + sha1 + 端标签」，落 `cache_dir/cf-files-index.json`。
//! - 只管官方包这一种：民间 CF/MCBBS 包字节在包里（解析层直接扫），`.mrpack` 清单自带 URL/sha1（走 downloader）。
//! - 直链不在这里取（带时效，构建期现取）；但「这一枚拿不拿得到字节」在这里探一次并落进同一张索引，构建前就得知道。
//! - 端标签（`gameVersions` 的 Client/Server，`cf_sides`）与元数据同一发响应带回：它是 CF 那侧
//!   最接近「模组自报端」的官方声明，贴回行上供 classify 播种 `EnvSource::CfFile`；老索引条目
//!   由 `env_unchecked_refs` 补问一轮（`env_checked` 有值后永久收队）。
//! - 补取只改写 `file_name`/`size_bytes`/`sha1`/端标签/许可态，**`path` 保持解析层的编号锚点不变**——detector 与任务存档都按它回指包内条目，名字一改锚点就飘。

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use futures::stream::{self, StreamExt};
use serde::Deserialize;

use super::downloader::{CfFileMeta, Downloader};
use super::parser::{CfRef, ParsedPack};
use crate::models::{CfLink, SideFlag};

/// 字面量单源在 `core::data_root`（那张表同时是卸载壳的删除清单）
const INDEX_FILE: &str = super::data_root::CACHE_CF_INDEX;

/// 一次在线轮里同时发几发。CF 门口有限流（经镜像同样存在），并发拉高只是把 429 提前；
/// 与 env 那侧的麦块档同宽（4）；官方反查那档已经抬到 16，这一档不跟着抬——它撞的是 CF 门口
/// 那条限流，不是 Modrinth 的宽额度。实测 495 行连发（并发 6）**零 429**，所以这一档有余量
const CONCURRENCY: usize = 4;

/// 一批多少行落一次盘：到点就写，超时/换包时已拿到的那部分不白要（env 侧同口径）
const CHUNK: usize = 32;

/// 整轮墙钟预算。**一条预算管两件事**：补元数据 + 探取链许可，所以从 60s 抬到 120s
/// （一行两次请求：一次 `/download-url` 的空 JSON、被拒时再一次 HEAD，都不下载字节）。
/// 单请求另有 `METADATA_TIMEOUT`（10s）掐着，这一档掐的是「几百行各慢一点」累出来的总账——
/// 补取跑在自动分类之前，用户是在等第一屏方案，不该让他在转圈里等十分钟
pub const ONLINE_BUDGET: Duration = Duration::from_secs(120);

/// 编号 → 索引键。`{mod}:{file}` 一段不多：同一模组的多个构建各占一行，与方案行一一对应
fn key_of(r: &CfRef) -> String {
    format!("{}:{}", r.mod_id, r.file_id)
}

/// `cache_dir/cf-files-index.json`：编号 → 那个构建的元数据。
///
/// `transparent`（与 `env-index.json` 同一坑位）：盘上存的就是这张表本身，不是 `{"map": …}`。
/// 少这一句，serde 读回来时外层键对不上、整表当未知键丢掉 ⇒ 写进去的元数据一条都读不回来，
/// 每次打开同一个 CF 包都要从零重打几百发请求
#[derive(Default, Deserialize)]
#[serde(transparent)]
pub struct CfIndex {
    map: HashMap<String, CfFileMeta>,
}

impl CfIndex {
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

    /// 这一枚编号补到了吗。没补到的行在界面上就叫编号，构建时也只能现取直链、无 sha1 可校验
    pub fn get(&self, r: &CfRef) -> Option<&CfFileMeta> {
        self.map.get(&key_of(r)).filter(|m| m.usable())
    }

    fn put(&mut self, r: &CfRef, m: CfFileMeta) {
        self.map.insert(key_of(r), m);
    }

    /// 这一枚编号探过取链许可了吗。`None` = 没探过（老索引条目、探的时候网络抖了）
    pub fn link_of(&self, r: &CfRef) -> Option<CfLink> {
        self.get(r).and_then(|m| m.link)
    }

    /// 把探测结论贴到**已有**的那条元数据上。没有元数据就不写：`link` 是顺带记在元数据行里的，
    /// 单存一条没名字的结论，enrich 时无处可贴，下次重解析也照旧看不见
    fn set_link(&mut self, r: &CfRef, link: CfLink) {
        if let Some(m) = self.map.get_mut(&key_of(r)) {
            m.link = Some(link);
        }
    }

    /// 把项目级聚合出的端标签升进内层（原值必须是「已问过、内层 None」）。
    /// 聚合答 None（项目里真没有一个文件勾过端标签）也升级：把「问过、没有」钉死，
    /// 否则每轮都对同一批行重发聚合请求
    pub fn upgrade_env(&mut self, r: &CfRef, sides: Option<(SideFlag, SideFlag)>) {
        if let Some(m) = self.map.get_mut(&key_of(r)) {
            if m.env_checked() && m.sides().is_none() {
                m.env = Some(sides);
            }
        }
    }

    /// 作废所有探过的许可态（「重新自动分类」那一档）。留着旧结论等于把这个包永久定型：
    /// 作者随时能开或关「通过 API 发放下载链」，而这条线上没有任何时间戳告诉我们它过期了
    pub fn clear_links(&mut self) {
        for m in self.map.values_mut() {
            m.link = None;
        }
    }
}

/// 这包里「按编号声明」的行数（= 是不是官方导出的 CF 包，以及构建时要联网取多少枚 jar）。
/// 名字可能早就被索引答过了、这一数照旧是全部 ⇒ 它是**包的属性**，不是某一轮的补取结果。
/// （历史上它还当过「缺 Key」提示的闸门，Key 随 mcimirror 免 Key 化退役后只剩计数一职）
pub fn cf_row_count(parsed: &ParsedPack) -> usize {
    parsed.mod_files.iter().filter(|f| f.cf.is_some()).count()
}

/// 还缺元数据的那些编号（按出现顺序去重）。索引命中的不再列入 ⇒ 第二次打开同一个包零请求
pub fn pending_refs(parsed: &ParsedPack, index: &CfIndex) -> Vec<CfRef> {
    let mut out: Vec<CfRef> = Vec::new();
    for f in &parsed.mod_files {
        let Some(r) = &f.cf else { continue };
        if index.get(r).is_none() && !out.contains(r) {
            out.push(r.clone());
        }
    }
    out
}

/// 把索引里的元数据贴回包内条目：**只改名字/大小/sha1/端标签/取链许可态，路径那枚锚点原样留着**（模块头有理由）
pub fn enrich(parsed: &ParsedPack, index: &CfIndex) -> ParsedPack {
    let mut out = parsed.clone();
    for f in &mut out.mod_files {
        let Some(cf) = &mut f.cf else { continue };
        let Some(m) = index.get(&*cf) else { continue };
        f.file_name = m.file_name.clone();
        f.size_bytes = m.size_bytes;
        f.sha1 = m.sha1.clone();
        // 构建级端标签（CF gameVersions 的 Client/Server）贴回行上：classify 播种
        // EnvSource::CfFile 证据读的就是它。没勾标签是 None——「没有声明」，不是「不支持」
        cf.env = m.sides();
        // 许可态贴回行上：构建前的闸门与方案行的「缺件」标记读的都是它（`detector` 据此写
        // `PlanMod.cf_blocked`），而那份行是要过一遍前端再回传进流水线的
        cf.link = m.link.unwrap_or_default();
    }
    out
}

/// 在线补取：逐枚编号要一次元数据，批末落盘。返回没补到的那些编号——已补到的照常进索引、
/// 照常生效。单个失败（网络抖动、编号不存在）只影响那一行，剩下的照常扫完
pub async fn resolve_online(
    dl: &Downloader,
    index: &mut CfIndex,
    cache_dir: &Path,
    refs: &[CfRef],
) -> Vec<CfRef> {
    let mut missing: Vec<CfRef> = Vec::new();
    for chunk in refs.chunks(CHUNK) {
        // 带着下标走：buffer_unordered 不保证完成序，而「哪枚编号」要能对上回来的那份元数据
        let queue: Vec<(usize, CfRef)> = chunk.iter().cloned().enumerate().collect();
        // `to_vec()` 那份拥有值的队列（按引用喂 buffer_unordered 会把 closure 绑死在一个寿命上）
        let done: Vec<(usize, Option<CfFileMeta>)> = stream::iter(queue)
            .map(|(i, r)| async move {
                match dl.curseforge_file_meta(&r.mod_id, &r.file_id).await {
                    Ok(m) if m.usable() => (i, Some(m)),
                    // 200 但没名字 / 请求失败：与查不到同档，整轮记「不完整」，别把空行贴进方案
                    _ => (i, None),
                }
            })
            .buffer_unordered(CONCURRENCY)
            .collect()
            .await;
        for (i, meta) in done {
            match meta {
                Some(m) => index.put(&chunk[i], m),
                None => missing.push(chunk[i].clone()),
            }
        }
        // 批末落盘：超时或换包打断时，已经拿到的那部分下次离线即答
        index.save(cache_dir);
    }
    missing
}

/// 还没探过取链许可的那些编号（去重）。只有**补到元数据**的行才进这一列：回落链是按
/// 文件名定位文件的，名字还没补回来的行既判不出 `Derived`/`Unavailable`，构建期也拿不到字节
pub fn unprobed_refs(parsed: &ParsedPack, index: &CfIndex) -> Vec<CfRef> {
    let mut out: Vec<CfRef> = Vec::new();
    for f in &parsed.mod_files {
        let Some(r) = &f.cf else { continue };
        if index.get(r).is_some() && index.link_of(r).is_none() && !out.contains(r) {
            out.push(r.clone());
        }
    }
    out
}

/// 文件级端标签**没勾**的那些编号（外层已问、内层 None）：作者上传那一份时没勾
/// Client/Server（老构建普遍如此），但 CF 网页上仍展示项目级「Environment」——
/// 从 mod 对象的 `latestFiles` 聚合（`curseforge_project_sides`）给这些行兜底。
/// 聚合答上的行把内层值升成 Some(Some(..))，下一次进同一个包不再聚合
pub fn env_unlabeled_refs(parsed: &ParsedPack, index: &CfIndex) -> Vec<CfRef> {
    let mut out: Vec<CfRef> = Vec::new();
    for f in &parsed.mod_files {
        let Some(r) = &f.cf else { continue };
        let unlabeled = index
            .get(r)
            .is_some_and(|m| m.env_checked() && m.sides().is_none());
        if unlabeled && !out.contains(r) {
            out.push(r.clone());
        }
    }
    out
}

/// 端标签还没问过的那些编号（去重）。只收**元数据已补到**的行：老索引条目（本功能上线前
/// 写进 `cf-files-index.json` 的那些）没有端标签这一问的记录，重发一次元数据请求把它补齐；
/// 补齐之后这条腿永久收队（`env_checked` 有值就不再来）。与 `unprobed_refs` 同一口径
pub fn env_unchecked_refs(parsed: &ParsedPack, index: &CfIndex) -> Vec<CfRef> {
    let mut out: Vec<CfRef> = Vec::new();
    for f in &parsed.mod_files {
        let Some(r) = &f.cf else { continue };
        let unchecked = index
            .get(r)
            .is_some_and(|m| !m.env_checked());
        if unchecked && !out.contains(r) {
            out.push(r.clone());
        }
    }
    out
}

/// 在线探测：逐枚问一次「拿不拿得到字节」，批末落盘。返回**探出结论的行数**。
/// 没有结论的那些（网络抖动）原样留白，下次再探
async fn probe_online(
    dl: &Downloader,
    index: &mut CfIndex,
    cache_dir: &Path,
    refs: &[CfRef],
) -> usize {
    let mut answered = 0usize;
    for chunk in refs.chunks(CHUNK) {
        // 名字在发请求之前就摘出来：闭包要读索引里的元数据，而写结论也在同一张表上，
        // 分开 borrow 才能让 buffer_unordered 的多个 future 同时活着
        let queue: Vec<(usize, CfRef, String)> = chunk
            .iter()
            .cloned()
            .enumerate()
            .map(|(i, r)| {
                let name = index.get(&r).map(|m| m.file_name.clone()).unwrap_or_default();
                (i, r, name)
            })
            .collect();
        let done: Vec<(usize, Option<CfLink>)> = stream::iter(queue)
            .map(|(i, r, name)| async move {
                (i, dl.curseforge_probe_link(&r.mod_id, &r.file_id, &name).await)
            })
            .buffer_unordered(CONCURRENCY)
            .collect()
            .await;
        for (i, link) in done {
            if let Some(l) = link {
                index.set_link(&chunk[i], l);
                answered += 1;
            }
        }
        // 批末落盘：与补元数据同一条预算，到点打断时已探出的那部分下次零请求
        index.save(cache_dir);
    }
    answered
}

/// 补取的结果：`parsed` 是把元数据与许可态贴回去的那一份；`unresolved` 仍然只有编号的行数
/// `renamed` = 贴完**真的改写了行**（名字/大小/sha1 任一）；
///
/// `renamed` 是调用方那两道闸门（换掉解析缓存那份 Arc、作废端取证结论）的判据，
/// 而且**不能拿「这轮发了几条请求」代替**：重启后重新解析拿到的是没补过的包，而索引是热的，
/// 那一档一条请求都不发、却必须改写——否则明明知道真名字也照样给用户一排编号。
/// 反过来索引空着且什么都没改时一条没改，就不该白重跑一整轮离线取证
pub struct Enriched {
    pub parsed: ParsedPack,
    pub unresolved: usize,
    pub renamed: bool,
    /// 这一轮改写**只动了取链许可态**（热索引第一次探测那一档）：一条元数据请求都不发、
    /// 名字早就补好了，但行上的 `link` 从 `Unknown` 变成结论 ⇒ 解析缓存那份 Arc 必须换
    /// （闸门与方案行的缺件标记读的都是它）。端取证结论与许可态无关，所以这一档**不该**作废它
    pub links_changed: bool,
    /// 这一轮把**端标签**贴到了以前没有的行上（老索引条目补问那一档）：与许可态同理，
    /// Arc 必须换（classify 播种 `EnvSource::CfFile` 读的是行上那枚），端取证结论不作废——
    /// 播种发生在它自己的阶梯里，不受影响
    pub env_changed: bool,
}

/// 一处入口，两处调用方（自动分类、构建阶段 1）共用：读索引 → 缺的联网补 → 补端标签 →
/// 探取链 → 贴回包内条目。
///
/// 五条省钱/省时口径：
/// - 索引热的那一档**元数据零请求**（同一个包第二次打开只读盘；但取链许可还是要探一次）
/// - 补元数据、补端标签与探取链**共用一条 `ONLINE_BUDGET`**（见那个常量），到点收摊，
///   已拿到的照常生效（每趟都批末落盘）
/// - 补端标签那条腿只为本功能上线前的老索引条目存在：`env_checked` 一旦有值就永久收队
/// - `unresolved` 在轮末**按索引现算**而不是取 `resolve_online` 的返回数：超时打断那一档
///   已经落盘的那部分是真补到了，报整批没补到是说谎
///
/// `reprobe=true`（「重新自动分类」）先把索引里的许可态作废再探一遍：结论是按当下的项目设置
/// 问出来的，作者随时能开关 API 发放，缓存它就得有一个明说的重探出口
pub async fn ensure(
    dl: &Downloader,
    cache_dir: &Path,
    parsed: &ParsedPack,
    reprobe: bool,
) -> Enriched {
    let mut index = CfIndex::load(cache_dir);
    if reprobe {
        index.clear_links();
    }
    let pending = pending_refs(parsed, &index);
    let _ = tokio::time::timeout(ONLINE_BUDGET, async {
        if !pending.is_empty() {
            resolve_online(dl, &mut index, cache_dir, &pending).await;
        }
        // 老索引条目补端标签：重发一次元数据（名字/大小/sha1 原样换新，多不了什么），
        // env_checked 从此有值，这条腿对这份索引就永久收队了
        let need_env = env_unchecked_refs(parsed, &index);
        if !need_env.is_empty() {
            resolve_online(dl, &mut index, cache_dir, &need_env).await;
        }
        // 文件级没勾标签的行：CF 网页的项目级「Environment」聚合兜底（latestFiles）。
        // 聚合答上/答不上都升级内层值，这条腿对这份索引同样永久收队
        let unlabeled = env_unlabeled_refs(parsed, &index);
        if !unlabeled.is_empty() {
            let ids: Vec<String> = unlabeled.iter().map(|r| r.mod_id.clone()).collect();
            let sides = dl.curseforge_project_sides(&ids).await;
            for r in &unlabeled {
                index.upgrade_env(r, sides.get(&r.mod_id).copied().flatten());
            }
            index.save(cache_dir);
        }
        let refs = unprobed_refs(parsed, &index);
        if !refs.is_empty() {
            probe_online(dl, &mut index, cache_dir, &refs).await;
        }
    })
    .await;
    let unresolved = pending_refs(parsed, &index).len();
    let out = enrich(parsed, &index);
    // enrich 不动行序，按下标对齐比一遍就知道到底改没改
    let mut renamed = false;
    let mut links_changed = false;
    let mut env_changed = false;
    for (a, b) in out.mod_files.iter().zip(parsed.mod_files.iter()) {
        if a.file_name != b.file_name || a.size_bytes != b.size_bytes || a.sha1 != b.sha1 {
            renamed = true;
        }
        if a.cf.as_ref().map(|r| r.link) != b.cf.as_ref().map(|r| r.link) {
            links_changed = true;
        }
        if a.cf.as_ref().map(|r| r.env) != b.cf.as_ref().map(|r| r.env) {
            env_changed = true;
        }
    }
    Enriched { parsed: out, unresolved, renamed, links_changed, env_changed }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::parser::PackFile;
    use crate::models::{LoaderKind, PackManifest};

    /// 一枚编号坐标（解析层出来的那一份：许可态恒 `Unknown`、required 恒按清单缺省走「必需」）
    fn cf_ref(mod_id: &str, file_id: &str) -> CfRef {
        CfRef {
            mod_id: mod_id.into(),
            file_id: file_id.into(),
            required: true,
            link: CfLink::Unknown,
            env: None,
        }
    }

    fn meta(file_name: &str, size_bytes: u64, sha1: Option<&str>) -> CfFileMeta {
        // env: Some(None) = 端标签这一问补过了、作者没勾：老索引补齐后的常态
        CfFileMeta { file_name: file_name.into(), size_bytes, sha1: sha1.map(String::from), env: Some(None), link: None }
    }

    fn row(mod_id: &str, file_id: &str) -> PackFile {
        let name = format!("{mod_id}-{file_id}.jar");
        PackFile {
            path: format!("mods/{name}"),
            file_name: name,
            url: String::new(),
            sha1: None,
            size_bytes: 0,
            in_pack: false,
            env_server: None,
            env_client: None,
            depends: Vec::new(),
            cf: Some(cf_ref(mod_id, file_id)),
        }
    }

    fn pack(mod_files: Vec<PackFile>) -> ParsedPack {
        ParsedPack {
            manifest: PackManifest {
                file_name: "p.zip".into(),
                loader: LoaderKind::Forge,
                mc_version: "1.20.1".into(),
                mod_count: mod_files.len() as u32,
                size_bytes: 0,
                parsed: true,
                error: None,
                source_path: None,
            },
            mod_files,
            extra_files: Vec::new(),
            loader_version: None,
            root_prefix: String::new(),
        }
    }

    /// 补取只动那三件事 + 许可态：路径锚点必须原样，否则存档里的 `src_path` 与重解析的行对不上
    #[test]
    fn enrich_rewrites_name_but_keeps_the_anchor() {
        let mut index = CfIndex::default();
        index.put(
            &cf_ref("1", "2"),
            meta("sodium-0.5.8.jar", 700_000, Some("ab")),
        );
        let p = pack(vec![row("1", "2"), row("9", "9")]);
        let out = enrich(&p, &index);
        assert_eq!(out.mod_files[0].file_name, "sodium-0.5.8.jar");
        assert_eq!(out.mod_files[0].size_bytes, 700_000);
        assert_eq!(out.mod_files[0].sha1.as_deref(), Some("ab"));
        assert_eq!(out.mod_files[0].path, "mods/1-2.jar", "锚点不许跟着名字飘");
        // 没补到的那行照旧是编号，不假装有名有大小
        assert_eq!(out.mod_files[1].file_name, "9-9.jar");
        assert_eq!(out.mod_files[1].size_bytes, 0);
    }

    /// 待查清单按索引冷热收缩：同一个包第二次进来应当零请求
    #[test]
    fn pending_refs_skips_what_the_index_already_answers() {
        let p = pack(vec![row("1", "2"), row("9", "9")]);
        let cold = CfIndex::default();
        assert_eq!(pending_refs(&p, &cold).len(), 2);
        let mut warm = CfIndex::default();
        warm.put(&cf_ref("1", "2"), meta("a.jar", 1, None));
        // 同一枚编号的重复行不该查第二遍
        let dup = pack(vec![row("1", "2"), row("1", "2"), row("9", "9")]);
        assert_eq!(pending_refs(&dup, &warm).len(), 1);
        assert_eq!(pending_refs(&dup, &warm)[0].file_id, "9");
    }

    /// 空名字 = 没补到（CF 个别构建真会给空 fileName），不能让方案行叫一个空文件名
    #[test]
    fn unusable_meta_is_not_an_answer() {
        let mut index = CfIndex::default();
        index.put(&cf_ref("1", "2"), CfFileMeta { file_name: "  ".into(), size_bytes: 9, sha1: None, env: Some(None), link: None });
        let p = pack(vec![row("1", "2")]);
        assert_eq!(pending_refs(&p, &index).len(), 1);
        assert_eq!(enrich(&p, &index).mod_files[0].file_name, "1-2.jar");
    }

    /// mrpack / 民间 CF 包（字节在包里）一行都不该联网
    #[test]
    fn packs_with_physical_jars_need_no_lookup() {
        let mut f = row("1", "2");
        f.cf = None;
        f.in_pack = true;
        let p = pack(vec![f]);
        assert_eq!(cf_row_count(&p), 0);
        assert!(pending_refs(&p, &CfIndex::default()).is_empty());
    }

    /// 探测结论也走 enrich 那一道贴回：闸门与方案行的缺件标记读的都是**行上**的那一枚
    #[test]
    fn enrich_pastes_the_probe_verdict() {
        let mut index = CfIndex::default();
        let mut m = meta("blocked-1-2.jar", 10, Some("ab"));
        m.link = Some(CfLink::Unavailable);
        index.put(&cf_ref("1", "2"), m);
        let out = enrich(&pack(vec![row("1", "2")]), &index);
        assert_eq!(out.mod_files[0].cf.as_ref().unwrap().link, CfLink::Unavailable);
        assert_eq!(out.mod_files[0].path, "mods/1-2.jar", "锚点照旧不许飘");
        // 索引没答许可态 ⇒ 行上仍是 Unknown，不是「拿不到」（把没探过说成缺件会拦下能构建的包）
        let fresh = enrich(&pack(vec![row("1", "2")]), &CfIndex::default());
        assert_eq!(fresh.mod_files[0].cf.as_ref().unwrap().link, CfLink::Unknown);
    }

    /// 探测清单按索引冷热收缩，且**只收有名字的行**：回落链按文件名定位，
    /// 名字还没补回来的行既判不出结论、构建期也拿不到字节
    #[test]
    fn unprobed_refs_only_takes_rows_with_metadata() {
        let p = pack(vec![row("1", "2"), row("9", "9")]);
        // 冷索引：一行元数据都没有 ⇒ 一行都不探（先补名字，下一轮再探）
        assert!(unprobed_refs(&p, &CfIndex::default()).is_empty());
        let mut index = CfIndex::default();
        index.put(&cf_ref("1", "2"), meta("a.jar", 1, None));
        assert_eq!(unprobed_refs(&p, &index).len(), 1);
        let mut probed = CfIndex::default();
        let mut m = meta("a.jar", 1, None);
        m.link = Some(CfLink::Derived);
        probed.put(&cf_ref("1", "2"), m);
        // 探过的行下次零请求
        assert!(unprobed_refs(&p, &probed).is_empty());
    }

    /// 「重新自动分类」作废的是许可态，不是元数据：清完仍然不需要补名字（零元数据请求），
    /// 但每一行都重新欠一次探测
    #[test]
    fn clear_links_empties_the_verdicts_not_the_metadata() {
        let mut index = CfIndex::default();
        let mut m = meta("a.jar", 1, None);
        m.link = Some(CfLink::Official);
        index.put(&cf_ref("1", "2"), m);
        let p = pack(vec![row("1", "2")]);
        assert_eq!(pending_refs(&p, &index).len(), 0, "补过名字就不该再发元数据请求");
        index.clear_links();
        assert_eq!(index.link_of(&cf_ref("1", "2")), None);
        assert_eq!(pending_refs(&p, &index).len(), 0, "元数据还活着");
        assert_eq!(unprobed_refs(&p, &index).len(), 1, "许可态清掉了才叫重探");
    }

    /// 端标签补问腿：只收**没问过**的老条目（`env` 外层 None），补过的（含「没勾」这个结论）
    /// 永不再问；贴回行上的端标签是 classify 播种 `EnvSource::CfFile` 的来源
    #[test]
    fn env_refetch_targets_legacy_rows_and_enrich_pastes_sides() {
        use crate::models::SideFlag;
        let mut index = CfIndex::default();
        let mut tagged = meta("oculus-1-2.jar", 10, Some("ab"));
        tagged.env = Some(Some((SideFlag::Required, SideFlag::Unsupported)));
        index.put(&cf_ref("1", "2"), tagged);
        // 老条目：env 外层 None = 本功能上线前写的，端标签还没问过
        index.put(
            &cf_ref("9", "9"),
            CfFileMeta {
                file_name: "legacy-9-9.jar".into(),
                size_bytes: 5,
                sha1: None,
                env: None,
                link: None,
            },
        );
        let p = pack(vec![row("1", "2"), row("9", "9")]);
        let need = env_unchecked_refs(&p, &index);
        assert_eq!(need.len(), 1, "只补问老条目，问过的不重问（没勾也是结论）");
        assert_eq!(need[0].mod_id, "9");
        // 端标签随 enrich 贴回行上；没问过的行保持 None（不是「不支持」）
        let out = enrich(&p, &index);
        assert_eq!(
            out.mod_files[0].cf.as_ref().unwrap().env,
            Some((SideFlag::Required, SideFlag::Unsupported))
        );
        assert_eq!(out.mod_files[1].cf.as_ref().unwrap().env, None);
    }

    /// 项目级聚合腿：只收「已问过、文件级没勾标签」的行；聚合答案（含 None）
    /// 升进内层后该行永久收队
    #[test]
    fn unlabeled_refs_and_upgrade_env_pin_the_verdict() {
        use crate::models::SideFlag;
        let mut index = CfIndex::default();
        // 已问过、没勾标签（内层 None）
        index.put(
            &cf_ref("1", "2"),
            CfFileMeta { file_name: "a.jar".into(), size_bytes: 5, sha1: None, env: Some(None), link: None },
        );
        // 连问都没问过（外层 None）
        index.put(
            &cf_ref("9", "9"),
            CfFileMeta { file_name: "b.jar".into(), size_bytes: 5, sha1: None, env: None, link: None },
        );
        let p = pack(vec![row("1", "2"), row("9", "9")]);
        let need = env_unlabeled_refs(&p, &index);
        assert_eq!(need.len(), 1, "只聚合「问过但没勾」的行");
        assert_eq!(need[0].mod_id, "1");

        // 聚合答上 → 升级内层，行上可读、收队
        index.upgrade_env(&cf_ref("1", "2"), Some((SideFlag::Required, SideFlag::Required)));
        assert!(env_unlabeled_refs(&p, &index).is_empty(), "升过级的不重问");
        let out = enrich(&p, &index);
        assert_eq!(
            out.mod_files[0].cf.as_ref().unwrap().env,
            Some((SideFlag::Required, SideFlag::Required))
        );

        // 聚合答 None（项目里真没人勾标签）→ 也钉死「问过、没有」
        index.upgrade_env(&cf_ref("9", "9"), None);
        assert!(env_unlabeled_refs(&p, &index).is_empty(), "答 None 也收队");
        assert_eq!(out.mod_files[1].cf.as_ref().unwrap().env, None, "None 不是声明");
    }

    /// 热索引：一轮下来名字照常贴回，但**一行缺件都不许冒出来**——索引答过的行不算缺件，
    /// 把「索引里有」演成「拿不到字节」等于用一个假状态拦下一个能构建的包
    #[tokio::test]
    async fn warm_index_ensure_invents_no_blocked_rows() {
        let dir = std::env::temp_dir().join(format!("sideshift-cf-keyless-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut index = CfIndex::default();
        index.put(&cf_ref("1", "2"), meta("sodium-0.5.8.jar", 700_000, Some("ab")));
        index.save(&dir);
        let dl = Downloader::new(dir.clone(), 1);
        let p = pack(vec![row("1", "2")]);
        for reprobe in [false, true] {
            let out = ensure(&dl, &dir, &p, reprobe).await;
            assert_eq!(out.unresolved, 0, "热索引不该报「仍只有编号」");
            assert!(out.renamed, "名字要贴回行上，不然界面照旧一排编号");
            assert!(!out.links_changed, "没探过就不该改写到许可态");
            assert_eq!(out.parsed.mod_files[0].cf.as_ref().unwrap().link, CfLink::Unknown);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

