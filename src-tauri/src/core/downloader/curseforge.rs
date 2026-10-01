//! CurseForge 侧：Core API v1 的搜索 / 构建列表 / 类别 / 指纹反查 / 临时下载链五类查询。
//! 所有请求经 mcimirror（`mod.mcimirror.top/curseforge`）免 Key 获取——那是无 Key 世界里唯一能应答
//! 的源（官方 API 除 `/categories` 外全 401），所以候选链里 CF API 只有镜像一条（`source::candidates`）。
//! CF 的端声明在**构建级**：file 对象 `gameVersions` 里的 `Client`/`Server` 标签是作者上传时
//! 勾的官方声明（实测 JEI 两侧齐勾、Oculus 等纯客户端只勾 Client）——按文件精确，但没有
//! optional 这一档，且老构建普遍没勾 ⇒ 没标签不算「服务端不支持」，只算这一层没答上（`cf_sides`）。
//! 下载链临时且可能缺：这里给出的 `url` 恒为空串，构建期按 (mod id, file id) 现取（`curseforge_build_url`：官方链优先、被拒退内容分发站推导链；`curseforge_probe_link` 逐枚探链入清单）。
//! 校验值按长度认（40=sha1、32=md5），不信 `algo` 枚举号（各语言实现公认对不上）。

use serde_json::Value;
use std::collections::HashMap;

use crate::core::mcmod_names;
use crate::models::{
    CfLink, LoaderKind, ModDepends, ModSearchPage, ModSearchQuery, ModSearchResult, ModSource,
    ModVersionEntry, SideFlag,
};
use super::client::{Downloader, OnceTable};
use super::types::DownloadError;
use super::util::urlencoding;

const CURSEFORGE_API: &str = "https://api.curseforge.com/v1";

/// Minecraft 在 CurseForge 的 game id
const MC_GAME_ID: u32 = 432;
/// `classId` = Mods。同一个 game 下还混着整合包/材质包/世界/插件，不带这一条会搜出一堆 modpack
const CLASS_MODS: u32 = 6;

/// 由构建编号推 CurseForge 内容分发站的直链：`files/{id/1000}/{id%1000}/{文件名}`，
/// 两段各补零到 4 位与 3 位（`8909889` → `files/8909/889/`）。
///
/// **官方没有文档**，`/download-url` 才是成文那条；但本机实测过 495 行的官方导出包：
/// 其中 6 行整项目被拒发 API 链（HTTP 403、空响应体，而元数据接口照回 200），
/// 走这条推导链拿到的字节 `长度 == fileLength`、头四字节 `504b0304`、
/// **sha1 与 API 声明值逐字节相等**。所以它只当回落用，且**必须有 sha1 当锚**：
/// 这条链按文件名定位文件，名字给错了它不报错，只给你另一枚 jar
pub fn cf_cdn_url(file_id: &str, file_name: &str) -> Option<String> {
    let id: u64 = file_id.trim().parse().ok()?;
    let name = file_name.trim();
    if name.is_empty() {
        return None;
    }
    let hi = id / 1000;
    let lo = id % 1000;
    let enc = urlencoding(name);
    Some(format!("https://edge.forgecdn.net/files/{hi:04}/{lo:03}/{enc}"))
}

/// 把 JSON 里的 id 读成字符串：CF 给数字（`id: 301445`），老接口个别给字符串，两种都得吃
fn ident(v: &Value) -> String {
    v.as_str()
        .map(String::from)
        .or_else(|| v.as_i64().map(|i| i.to_string()))
        .unwrap_or_default()
}

/// 数字字段（缺失/类型不对都当 0，不让一整页搜索结果因为一条脏数据报错）
fn num(v: &Value) -> u64 {
    v.as_u64().or_else(|| v.as_i64().map(|i| i as u64)).unwrap_or(0)
}

/// CurseForge 的加载器档位号（`ModLoaderType`：1=Forge、4=Fabric、6=NeoForge）
fn cf_loader_type(loader: LoaderKind) -> u32 {
    match loader {
        LoaderKind::Forge => 1,
        LoaderKind::Fabric => 4,
        LoaderKind::NeoForge => 6,
    }
}

/// 从 `hashes[]` 里挑 sha1：按 40 位 hex 认，不看 `algo` 号（模块头解释了为什么）
fn cf_sha1(file: &Value) -> Option<String> {
    file["hashes"].as_array()?.iter().find_map(|h| {
        let v = h["value"].as_str()?.trim().to_ascii_lowercase();
        (v.len() == 40 && v.chars().all(|c| c.is_ascii_hexdigit())).then_some(v)
    })
}

/// 一条构建的元数据，即 CF 清单**没写、只能联网要**的那三件事。
/// 落 `cache_dir/cf-files-index.json` 时要序列化，所以 Serialize 也在这里挂上
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CfFileMeta {
    pub file_name: String,
    pub size_bytes: u64,
    pub sha1: Option<String>,
    /// 构建级端标签（`cf_sides` 的产出）。**双层 Option 是给老索引留的升级通道**：
    /// 外层 `None` = 这条是本功能上线前写进索引的（没问过端标签）⇒ `cfpack` 会为它
    /// 重发一次元数据请求；`Some(None)` = 问过了、作者两个标签都没勾（这也是个结论，
    /// 不许再问）；`Some(Some((c, s)))` = 问到了真声明。老条目经一轮补取后都有了外层值，
    /// 「同一个包第二次打开零请求」从那之后恢复
    #[serde(default)]
    pub env: Option<Option<(SideFlag, SideFlag)>>,
    /// 索引顺带记的**取链许可态**（不属于 API 那三件事，是本地探出来的）。
    /// `None` = 还没探过（老索引条目、网络抖动没答上的那些轮）；探过的行下次进同一个包零请求。
    /// 存的是「能不能拿到链」这个结论，**不是链本身**——直链带时效，存下来就是埋雷
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<CfLink>,
}

impl CfFileMeta {
    /// 补取是否有效：没名字等于没补到，不能让方案行叫一个空文件名
    pub fn usable(&self) -> bool {
        !self.file_name.trim().is_empty()
    }

    /// 端标签这一问补过了没有（外层有值即问过，无论内层是不是 None）
    pub fn env_checked(&self) -> bool {
        self.env.is_some()
    }

    /// 贴回 `CfRef` 的那份端声明：问过才有值，没勾标签就是 None
    pub fn sides(&self) -> Option<(SideFlag, SideFlag)> {
        self.env.flatten()
    }
}

/// 一条指纹匹配带回的东西：那枚 file 的端标签与 sha1（有的行顺手把 sha1 也补上，
/// 证据可以同时挂 `fp:` 与 `sha1:` 两个键，下次离线即答）
#[derive(Debug, Clone)]
pub struct CfFpMatch {
    pub sides: Option<(SideFlag, SideFlag)>,
    pub sha1: Option<String>,
}

/// 从 `gameVersions` 标签里挑 MC 版本号。CF 这一个数组同时装着加载器名、
/// 端标签（"Client/Server"）和 "Java Edition" 之类的分类，只认真正长得像版本号的那个
fn cf_mc_version(file: &Value) -> String {
    file["gameVersions"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|g| g.as_str())
                .find(|g| {
                    let t = g.trim();
                    t.len() >= 4
                        && t.starts_with("1.")
                        && t.chars().next().is_some_and(|c| c.is_ascii_digit())
                        && t.chars()
                            .all(|c| c.is_ascii_digit() || c == '.' || c == '_')
                })
                .unwrap_or_default()
                .to_string()
        })
        .unwrap_or_default()
}

/// 从 `gameVersions` 标签里认加载器。先判 NeoForge：它的名字里含 "forge"，顺序反了会被认成 Forge。
/// 认不出来落 Fabric —— 与 Modrinth 侧同一档兜底（`list_mod_versions` 的 `unwrap_or(LoaderKind::Fabric)`）
fn cf_loader(file: &Value) -> LoaderKind {
    let hit = |needle: &str| {
        file["gameVersions"]
            .as_array()
            .map(|a| a.iter().filter_map(|g| g.as_str()).any(|g| g.eq_ignore_ascii_case(needle)))
            .unwrap_or(false)
    };
    if hit("neoforge") {
        LoaderKind::NeoForge
    } else if hit("fabric") || hit("quilt") {
        LoaderKind::Fabric
    } else if hit("forge") {
        LoaderKind::Forge
    } else {
        LoaderKind::Fabric
    }
}

/// 构建级端标签（`gameVersions` 里的 `Client`/`Server`）→ 两侧支持度。
///
/// **四个象限的判据来自 2026-10 实测**（api.cfwidget.com 的文件级 `versions` 标签计数，
/// 与 API 的 `gameVersions` 同源）：JEI（两端可用）的 Client 与 Server 各 1059 个文件、
/// 逐文件齐勾；Oculus / Legendary Tooltips / Mouse Tweaks 三个纯客户端模组
/// 全部只勾 Client、Server 为 0。即作者勾了哪侧就是哪侧可用：
/// - 只勾 `Client` ⇒ (必需, 不支持)——纯客户端模组在 CF 上的标准勾法；
/// - 只勾 `Server` ⇒ (不支持, 必需)，两侧齐勾 ⇒ (必需, 必需)；
/// - **两个都没勾 ⇒ `None`**：老构建普遍没勾，把「没勾」读成「不支持」会把
///   那些模组从服务端包里冤枉删掉——这一层没答上，交回证据阶梯的下一层
fn cf_sides(file: &Value) -> Option<(SideFlag, SideFlag)> {
    let hit = |tag: &str| {
        file["gameVersions"]
            .as_array()
            .map(|a| a.iter().filter_map(|g| g.as_str()).any(|g| g.eq_ignore_ascii_case(tag)))
            .unwrap_or(false)
    };
    use SideFlag::{Required, Unsupported};
    match (hit("Client"), hit("Server")) {
        (true, true) => Some((Required, Required)),
        (true, false) => Some((Required, Unsupported)),
        (false, true) => Some((Unsupported, Required)),
        (false, false) => None,
    }
}

/// file 的 `dependencies[]` → 前置表。CF 的关系号（`ModDependencyType`）：
/// 3 = 必需前置、2 = 可选前置；1 = 内嵌库、4 = 工具、5 = 不兼容、6 = 包含
/// 都不是「要另装的前置」，不进表。同一 modId 重复声明时按「任一必需」记
fn cf_depends(f: &Value) -> Vec<ModDepends> {
    let mut out: Vec<ModDepends> = Vec::new();
    for d in f["dependencies"].as_array().map(Vec::as_slice).unwrap_or_default() {
        let id = ident(&d["modId"]);
        if id.is_empty() || id == "0" {
            continue;
        }
        let required = match d["relationType"].as_u64() {
            Some(3) => true,
            Some(2) => false,
            _ => continue,
        };
        match out.iter_mut().find(|x| x.id == id) {
            Some(x) => x.required = x.required || required,
            None => out.push(ModDepends {
                id,
                name: None,
                name_zh: None,
                slug: None,
                required,
            }),
        }
    }
    out
}

/// 一条 CF file → 一个构建条目。`url` 恒空：见模块头第 3 条
fn file_entry(f: &Value, want_mc: &str) -> Option<ModVersionEntry> {
    // CF 上「处理中/被拒/已删」的文件 `isAvailable=false`，点了必失败，不如根本不给
    if f["isAvailable"].as_bool() == Some(false) {
        return None;
    }
    let id = ident(&f["id"]);
    if id.is_empty() {
        return None;
    }
    let file_name = f["fileName"].as_str().unwrap_or("mod.jar").to_string();
    Some(ModVersionEntry {
        version_number: f["displayName"]
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(&file_name)
            .to_string(),
        mc_version: if want_mc.is_empty() {
            cf_mc_version(f)
        } else {
            want_mc.to_string()
        },
        loader: cf_loader(f),
        date: f["fileDate"]
            .as_str()
            .map(|s| s.chars().take(10).collect())
            .unwrap_or_default(),
        size_bytes: num(&f["fileLength"]),
        recommended: false, // 排序后由调用方标首个
        url: String::new(),
        sha1: cf_sha1(f),
        file_name,
        // 构建级端标签：作者勾的 Client/Server，没勾就留 None（`cf_sides` 有实测判据）
        client_side: cf_sides(f).map(|(c, _)| c),
        server_side: cf_sides(f).map(|(_, s)| s),
        depends: cf_depends(f),
        id,
    })
}

/// 组装搜索 URL。`modLoaderType` 只在同时带了 `gameVersion` 时才发（平台要求二者搭配，
/// 只发前者实测会被忽略 —— 「全部版本 + 指定加载器」这一格在 CF 侧做不到服务端筛选）
fn cf_search_url(q: &ModSearchQuery, page_size: u32) -> String {
    let offset = q.page.saturating_sub(1) * page_size;
    let mut url = format!(
        "{CURSEFORGE_API}/mods/search?gameId={MC_GAME_ID}&classId={CLASS_MODS}\
         &pageSize={page_size}&index={offset}&sortField={}&sortOrder=desc",
        if q.text.trim().is_empty() { 2 } else { 13 } // 2=Popularity（下载量） 13=Relevancy
    );
    if !q.text.trim().is_empty() {
        url.push_str(&format!("&searchFilter={}", urlencoding(q.text.trim())));
    }
    if !q.mc_version.is_empty() {
        url.push_str(&format!("&gameVersion={}", urlencoding(&q.mc_version)));
        if let Some(l) = q.loader {
            url.push_str(&format!("&modLoaderType={}", cf_loader_type(l)));
        }
    }
    url
}

impl Downloader {
    /// CF 的类别表（**免 Key**）：模组向 `(号, 名字)`，按名字排序去重。
    /// 下拉给用户看名字、搜索接口只认 `categoryIds` 号，所以这一张表同时喂两处
    async fn cf_categories(&self) -> Result<Vec<(u64, String)>, DownloadError> {
        static CF_CATEGORIES: OnceTable<(u64, String)> = OnceTable::new();
        CF_CATEGORIES
            .memo(|| async move {
                let url = format!("{CURSEFORGE_API}/categories?gameId={MC_GAME_ID}");
                let v = self.get_json(&url).await?;
                let mut out: Vec<(u64, String)> = v["data"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter(|c| c["classId"].as_i64() == Some(CLASS_MODS as i64))
                            .filter_map(|c| {
                                let id = num(&c["id"]);
                                let name = c["name"].as_str()?.trim().to_string();
                                (id > 0 && !name.is_empty()).then_some((id, name))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                out.sort_by(|a, b| a.1.cmp(&b.1));
                out.dedup_by(|a, b| a.1 == b.1);
                Ok(out)
            })
            .await
    }

    /// 「类别」下拉的选项（名字）；`search_curseforge` 再把选中的名字换回号
    pub async fn list_curseforge_categories(&self) -> Result<Vec<String>, DownloadError> {
        Ok(self.cf_categories().await?.into_iter().map(|(_, n)| n).collect())
    }

    /// 搜索模组（结果里的 id 是 CF 的 mod id 数字串，与 Modrinth 的 slug 同栏位、不同词表）
    pub async fn search_curseforge(
        &self,
        q: &ModSearchQuery,
    ) -> Result<ModSearchPage, DownloadError> {
        let page_size = 20u32;
        let mut url = cf_search_url(q, page_size);
        // 下拉的选项就来自这张表，所以换不到号只可能是「切了来源、选择还没落地」的一瞬间；
        // 那种情况按不设类别筛选处理，不能让一次搜索因为一个过期的选择整个失败
        let want_cat = q
            .category
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty() && *s != "all");
        if let Some(name) = want_cat {
            if let Some((id, _)) = self
                .cf_categories()
                .await?
                .into_iter()
                .find(|(_, n)| n.eq_ignore_ascii_case(name))
            {
                url.push_str(&format!("&categoryIds={id}"));
            }
        }
        let v = self.get_json(&url).await?;
        let mut results = Vec::new();
        if let Some(arr) = v["data"].as_array() {
            for m in arr {
                let id = ident(&m["id"]);
                if id.is_empty() {
                    continue;
                }
                results.push(ModSearchResult {
                    slug: m["slug"].as_str().map(String::from),
                    name: m["name"].as_str().unwrap_or_default().to_string(),
                    name_zh: mcmod_names::zh_name_of(m["slug"].as_str()),
                    description: m["summary"].as_str().unwrap_or_default().to_string(),
                    author: m["authors"]
                        .as_array()
                        .and_then(|a| a.first())
                        .and_then(|a| a["name"].as_str())
                        .unwrap_or_default()
                        .to_string(),
                    downloads: num(&m["downloadCount"]),
                    icon_url: m["logo"]["thumbnailUrl"]
                        .as_str()
                        .or(m["logo"]["url"].as_str())
                        .map(String::from),
                    source: ModSource::Curseforge,
                    compatible: true,
                    already_added: false, // 由 commands 层按当前方案回填
                    // CF 不给端声明：这里留 None，界面上就不挂端标签（而不是挂一个猜出来的）
                    client_side: None,
                    server_side: None,
                    id,
                });
            }
        }
        Ok(ModSearchPage {
            source: ModSource::Curseforge,
            total: v["pagination"]["totalCount"].as_u64().unwrap_or(0),
            results,
            page: q.page,
            page_size,
        })
    }

    /// 某模组的可用构建（新→旧，兼容请求版本的排前，首个标推荐）
    pub async fn list_curseforge_versions(
        &self,
        mod_id: &str,
        mc_version: &str,
    ) -> Result<Vec<ModVersionEntry>, DownloadError> {
        let mut url = format!("{CURSEFORGE_API}/mods/{mod_id}/files?pageSize=50");
        if !mc_version.is_empty() {
            url.push_str(&format!("&gameVersion={}", urlencoding(mc_version)));
        }
        let v = self.get_json(&url).await?;
        let mut entries: Vec<ModVersionEntry> = v["data"]
            .as_array()
            .map(|a| a.iter().filter_map(|f| file_entry(f, mc_version)).collect())
            .unwrap_or_default();
        // 前置反查：全部构建里出现过的 modId 去重后**一次批量**问名（`POST /v1/mods`，
        // 上限 100——超出的那条前置只剩数字 id，前端按 id 兜底）。请求失败就缺省（前置行
        // 是顺手的显示件，不是主内容），但不因它失败
        let mut ids: Vec<String> = entries
            .iter()
            .flat_map(|e| e.depends.iter().map(|d| d.id.clone()))
            .collect();
        ids.sort();
        ids.dedup();
        ids.truncate(100);
        let briefs = self.curseforge_mod_briefs(&ids).await;
        for e in entries.iter_mut() {
            for d in e.depends.iter_mut() {
                if let Some((slug, name)) = briefs.get(&d.id) {
                    d.slug = (!slug.is_empty()).then(|| slug.clone());
                    d.name = (!name.is_empty()).then(|| name.clone());
                }
                // 前置胶囊也是模组名：slug 到手就顺手查一次内置词典（离线、零请求）
                d.name_zh = mcmod_names::zh_name_of(d.slug.as_deref());
            }
        }
        // 运行时基建（Fabric API / QSL）不当前置展示：判据见 `ModDepends::is_runtime_base`。
        // 放在名字回填之后，slug 那一档判据才有得比
        for e in entries.iter_mut() {
            e.depends.retain(|d| !d.is_runtime_base());
        }
        entries.sort_by(|a, b| {
            (a.mc_version != mc_version)
                .cmp(&(b.mc_version != mc_version))
                .then(b.date.cmp(&a.date))
        });
        if let Some(first) = entries.first_mut() {
            first.recommended = true;
        }
        Ok(entries)
    }

    /// 一批 CF mod id → `(slug, name)`（`POST /v1/mods`，body `{"modIds": [数字]}`）。
    /// 给「前置模组」显示挂名用：请求失败返回空表（调用方按数字 id 兜底），不算故障
    pub async fn curseforge_mod_briefs(
        &self,
        ids: &[String],
    ) -> HashMap<String, (String, String)> {
        let mut out = HashMap::new();
        let nums: Vec<u64> = ids.iter().filter_map(|s| s.trim().parse().ok()).collect();
        if nums.is_empty() {
            return out;
        }
        if let Ok(v) = self
            .post_json(
                &format!("{CURSEFORGE_API}/mods"),
                &serde_json::json!({ "modIds": nums }),
            )
            .await
        {
            if let Some(arr) = v["data"].as_array() {
                for m in arr {
                    let id = ident(&m["id"]);
                    if id.is_empty() {
                        continue;
                    }
                    out.insert(
                        id,
                        (
                            m["slug"].as_str().unwrap_or_default().to_string(),
                            m["name"].as_str().unwrap_or_default().to_string(),
                        ),
                    );
                }
            }
        }
        out
    }

    /// 单个项目的展示信息（`GET /v1/mods/{modId}`）：详情页直跳一枚前置时补齐详情页要的
    /// 那几样，作者/简介/图标/下载量都在这一发响应里。CF 没有项目级端声明，两侧恒空——
    /// 详情页的端标签会退到版本行那份构建级标签（`file_entry`），与搜索结果同一口径
    pub async fn curseforge_detail(
        &self,
        mod_id: &str,
    ) -> Result<ModSearchResult, DownloadError> {
        let url = format!("{CURSEFORGE_API}/mods/{}", urlencoding(mod_id.trim()));
        let v = self.get_json(&url).await?;
        let m = &v["data"];
        let id = ident(&m["id"]);
        if id.is_empty() {
            return Err(DownloadError::NotFound(format!(
                "CurseForge 模组 {mod_id}"
            )));
        }
        Ok(ModSearchResult {
            id,
            slug: m["slug"].as_str().map(String::from),
            name: m["name"].as_str().unwrap_or_default().to_string(),
            name_zh: mcmod_names::zh_name_of(m["slug"].as_str()),
            description: m["summary"].as_str().unwrap_or_default().to_string(),
            author: m["authors"]
                .as_array()
                .and_then(|a| a.first())
                .and_then(|a| a["name"].as_str())
                .unwrap_or_default()
                .to_string(),
            downloads: num(&m["downloadCount"]),
            icon_url: m["logo"]["thumbnailUrl"]
                .as_str()
                .or(m["logo"]["url"].as_str())
                .map(String::from),
            source: ModSource::Curseforge,
            compatible: true,
            already_added: false,
            client_side: None,
            server_side: None,
        })
    }

    /// 一条声明编号 → 那个构建的**离线拿不到的三件事**（文件名 / 大小 / sha1）。
    ///
    /// CF 的 `files[]` 只给 `projectID`/`fileID`，方案行因此连名字都是编号；这一档把名字补回来，
    /// 顺带给取件档（`curseforge_download_url`）一条能校验的 sha1。
    /// 走 `POST /v1/mods/files`（传单元素数组）：**单文件 GET 端点经 mcimirror 502**（2026-10 实测），
    /// 批量 POST 两侧都可用、响应是同一份 file 对象——原来「没 Key 验不了批量口」的顾虑随
    /// Key 一起退役了。以后要省请求量，这里天然可以扩成真正的批量
    pub async fn curseforge_file_meta(
        &self,
        mod_id: &str,
        file_id: &str,
    ) -> Result<CfFileMeta, DownloadError> {
        let url = format!("{CURSEFORGE_API}/mods/files");
        let v = self
            .post_json(
                &url,
                &serde_json::json!({ "fileIds": [file_id.trim().parse::<u64>().unwrap_or(0)] }),
            )
            .await?;
        let f = &v["data"][0];
        // 没 id 就等于没认出来：CF 的 404/空 data 都会落在这上面，不能让一行空行进方案
        if ident(&f["id"]).is_empty() || ident(&f["modId"]) != mod_id {
            return Err(DownloadError::NotFound(format!(
                "CurseForge 构建 {mod_id}/{file_id} 的元数据"
            )));
        }
        Ok(CfFileMeta {
            file_name: f["fileName"].as_str().unwrap_or_default().trim().to_string(),
            size_bytes: num(&f["fileLength"]),
            sha1: cf_sha1(f),
            // 端标签与元数据同一发响应：外层 Some = 这一问补过了（没勾标签是 Some(None)，也是结论）
            env: Some(cf_sides(f)),
            // 取链许可是**另一发请求**的结论，不在这条元数据里（CF 没有任何字段预告
            // 「这个项目放不放行 API 链」，见 `CfLink` 的文档）。留 None = 还没探过
            link: None,
        })
    }

    /// 现取一条构建的下载直链。返回的 URL 带时效，所以只在构建期调用、不落档不缓存。
    /// 响应按官方文档是 `{data: "<url>"}`；个别版本给 `{data:{downloadUrl}}` → 两种都认
    pub async fn curseforge_download_url(
        &self,
        mod_id: &str,
        file_id: &str,
    ) -> Result<String, DownloadError> {
        let url = format!("{CURSEFORGE_API}/mods/{mod_id}/files/{file_id}/download-url");
        let v = self.get_json(&url).await?;
        let link = v["data"]
            .as_str()
            .map(String::from)
            .or_else(|| v["data"]["downloadUrl"].as_str().map(String::from))
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| {
                DownloadError::NotFound(format!("CurseForge 构建 {mod_id}/{file_id} 的下载链接"))
            })?;
        Ok(link)
    }

    /// 一条构建编号 → **拿得到字节**的 URL：官方直链优先，项目不放行时才退到内容分发站的推导链。
    ///
    /// 为什么不直接退：官方那条是唯一有文档背书的。为什么可以退：实测过（见 `cf_cdn_url`）。
    /// 闸门只剩一道——**手里有 sha1**（推导链按文件名定位，文件名错了它不报错，只给你另一枚 jar）。
    /// 「配了 Key」那道随 Key 一起退役：CDN 推导链免 Key 可达，元数据经镜像也必能补到 sha1
    pub async fn curseforge_build_url(
        &self,
        mod_id: &str,
        file_id: &str,
        file_name: &str,
        sha1: Option<&str>,
    ) -> Result<String, DownloadError> {
        match self.curseforge_download_url(mod_id, file_id).await {
            Ok(link) => Ok(link),
            Err(e) => {
                let anchored = sha1.map(|s| !s.trim().is_empty()).unwrap_or(false);
                let cdn = if anchored { cf_cdn_url(file_id, file_name) } else { None };
                match cdn {
                    Some(u) => Ok(u),
                    None => Err(e),
                }
            }
        }
    }

    /// 探一枚构建**到底拿不拿得到字节**：自动分类那一轮跑，构建期不再问。
    ///
    /// 官方链一问，被拒时再多一次 HEAD：
    /// - `/download-url` 给链 ⇒ `Official`
    /// - 被拒 / 答「没有这条链」⇒ 试内容分发站的推导链，HEAD 有正长度 = `Derived`、明确没有 = `Unavailable`
    /// - 连不上 / 超时 / 429 / 5xx ⇒ **`None`（不落档）**：这一档没有回答，把一次网络抖动写成
    ///   「这枚模组永久拿不到」等于永久拦下一个本来能构建的包
    ///
    /// 为什么不离线判定：CF 的 file 对象里没有任何「允许不允许 API 发放下载链」的字段——
    /// 实测 495 行的官方导出包，被整项目 403 拒掉的那 6 行 `isAvailable=true`、`fileStatus=4`、
    /// `externalLink=""`，与正常行一模一样（见 `CfLink`）。403 会从镜像原样透传回来，
    /// 所以「项目不放行」这一档在免 Key 世界里依然存在、依然要走 CDN 回落
    pub async fn curseforge_probe_link(
        &self,
        mod_id: &str,
        file_id: &str,
        file_name: &str,
    ) -> Option<CfLink> {
        match self.curseforge_download_url(mod_id, file_id).await {
            Ok(_) => Some(CfLink::Official),
            // 只有「平台答了、答的是不给」才值得去看回落链；`Http`（429/5xx/断连）没有结论。
            // 403 是「项目不放行」的透传（经镜像与带 Key 直连同形），归这一档
            Err(DownloadError::Http { status: 403, .. })
            | Err(DownloadError::NotFound(_)) => {
                let url = cf_cdn_url(file_id, file_name)?;
                match self.cf_head_answer(&url).await {
                    Some(true) => Some(CfLink::Derived),
                    Some(false) => Some(CfLink::Unavailable),
                    None => None,
                }
            }
            Err(_) => None,
        }
    }

    /// 按**文件指纹**批量反查构建（`POST /v1/fingerprints/{MC_GAME_ID}`）。
    /// zip 包内的 CF 独占模组（Forge 系大多不在 Modrinth）既没有 sha1 可查 Modrinth、
    /// 清单又不给编号，murmur2 指纹（`env::jar::cf_fingerprint`）是把「这枚 jar」
    /// 对回 CF 官方文件对象的唯一身份钥匙；匹配上的行带完整 file 对象——端标签（`cf_sides`）
    /// 与 sha1（`cf_sha1`）都从那里来。没匹配上的指纹直接缺席，不算故障。
    /// `Err` = 镜像不可达等网络故障，由调用方计进「这一轮没跑完」（无 Key 世界里没有
    /// 「缺凭据被拒」这一档了）。
    /// 官方接口单批上限 128，这里取 100 留余量
    pub async fn curseforge_fingerprints(
        &self,
        fingerprints: &[u32],
    ) -> Result<HashMap<u32, CfFpMatch>, DownloadError> {
        let mut out = HashMap::new();
        if fingerprints.is_empty() {
            return Ok(out);
        }
        let url = format!("{CURSEFORGE_API}/fingerprints/{MC_GAME_ID}");
        let v = self
            .post_json(&url, &serde_json::json!({ "fingerprints": fingerprints }))
            .await?;
        if let Some(arr) = v["data"]["exactMatches"].as_array() {
            for m in arr {
                let fp = num(&m["id"]) as u32;
                if fp == 0 {
                    continue;
                }
                let f = &m["file"];
                out.insert(
                    fp,
                    CfFpMatch {
                        sides: cf_sides(f),
                        sha1: cf_sha1(f),
                    },
                );
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::SideFlag;
    use LoaderKind::*;

    /// 端标签四象限（实测判据见 `cf_sides` 注释）：勾了哪侧才是哪侧，
    /// **两个都没勾 ≠ 服务端不支持**——那批老构建交回证据阶梯的下一层
    #[test]
    fn cf_sides_reads_only_what_the_author_tagged() {
        let gv = |tags: &[&str]| serde_json::json!({ "gameVersions": tags });
        use SideFlag::{Required, Unsupported};
        assert_eq!(
            cf_sides(&gv(&["1.20.1", "Forge", "Client", "Server"])),
            Some((Required, Required))
        );
        assert_eq!(
            cf_sides(&gv(&["1.20.1", "Client"])),
            Some((Required, Unsupported))
        );
        assert_eq!(
            cf_sides(&gv(&["1.20.1", "Server"])),
            Some((Unsupported, Required))
        );
        assert_eq!(cf_sides(&gv(&["1.20.1", "Forge"])), None);
        assert_eq!(cf_sides(&gv(&[])), None);
    }

    /// 关系号只收 3（必需）/ 2（可选）；内嵌库、工具、不兼容、包含都不算「要另装的前置」；
    /// modId 0 与重复声明（任一必需）也有各自的判
    #[test]
    fn cf_depends_keeps_only_installable_relations() {
        let f = serde_json::json!({
            "dependencies": [
                { "modId": 238222, "relationType": 3 },
                { "modId": 238222, "relationType": 2 },
                { "modId": 456, "relationType": 2 },
                { "modId": 789, "relationType": 1 },
                { "modId": 111, "relationType": 5 },
                { "modId": 0, "relationType": 3 },
                { "relationType": 3 }
            ]
        });
        let got = cf_depends(&f);
        assert_eq!(got.len(), 2, "内嵌/不兼容/脏行不进表");
        assert_eq!(got[0].id, "238222");
        assert!(got[0].required, "任一必需即必需");
        assert_eq!(got[1].id, "456");
        assert!(!got[1].required);
        assert_eq!(cf_depends(&serde_json::json!({})).len(), 0);
    }

    /// 回落链那两段的补零是这条规则唯一容易写错的地方：`8797042` → `8797/042`，不是 `8797/42`。
    /// 号码超过四位时只补不截（`12345678` → `12345/678`）
    #[test]
    fn derived_cdn_url_pads_both_segments() {
        assert_eq!(
            cf_cdn_url("8909889", "modelfix-1.21-1.10.jar").unwrap(),
            "https://edge.forgecdn.net/files/8909/889/modelfix-1.21-1.10.jar"
        );
        assert!(cf_cdn_url("8797042", "a.jar")
            .unwrap()
            .contains("/files/8797/042/"));
        assert!(cf_cdn_url("12345678", "a.jar")
            .unwrap()
            .contains("/files/12345/678/"));
    }

    /// 名字还没补到（那一轮没查完）就不给链：这条链按**文件名**定位文件，
    /// 名字错了它不报错，只给你另一枚 jar
    #[test]
    fn derived_cdn_url_requires_a_name_and_a_numeric_id() {
        assert!(cf_cdn_url("8909889", "").is_none());
        assert!(cf_cdn_url("8909889", "   ").is_none());
        assert!(cf_cdn_url("", "a.jar").is_none());
        assert!(cf_cdn_url("mods/44/a.jar", "a.jar").is_none());
    }

    #[test]
    fn derived_cdn_url_percent_encodes_the_file_name() {
        assert!(cf_cdn_url("5591286", "Structory 26.2 v1.3.7.jar")
            .unwrap()
            .ends_with("/Structory%2026.2%20v1.3.7.jar"));
    }

    /// 一条真形状的 CF 搜索结果（字段取自官方 OpenAPI 的 `Mod`/`File`），用来钉死映射口径
    const FIXTURE: &str = r#"{
      "pagination": { "index": 0, "pageSize": 20, "resultCount": 1, "totalCount": 4321 },
      "data": [
        {
          "id": 301445, "gameId": 432, "name": "YetAnotherConfigLib", "slug": "yacl",
          "summary": "A config library made with modern frameworks",
          "downloadCount": 8123456,
          "authors": [ { "id": 1, "name": "XanderIsDeviant" } ],
          "logo": { "thumbnailUrl": "https://media.forgecdn.net/avatars/1/2/logo.png" },
          "classId": 6,
          "latestFiles": [
            { "id": 4001, "displayName": "YACL 1.20.1", "fileName": "yacl-1.20.1.jar",
              "isAvailable": true, "fileLength": 512000, "fileDate": "2023-11-02T10:00:00Z",
              "gameVersions": ["1.20.1", "Fabric", "Client/Server"],
              "hashes": [ {"value": "0F1E2D3C4B5A69788796A5B4C3D2E1F0A9B8C7D6", "algo": 2} ],
              "downloadUrl": null }
          ]
        }
      ]
    }"#;

    /// 一条「不能用」的构建：处理中（isAvailable=false）
    const UNAVAILABLE: &str = r#"{ "id": 4002, "displayName": "", "fileName": "x.jar",
        "isAvailable": false, "gameVersions": ["1.20.1"], "hashes": [] }"#;

    #[test]
    fn cf_search_url_encodes_filters_and_paging() {
        let q = ModSearchQuery {
            source: ModSource::Curseforge,
            text: "sodium".into(),
            mc_version: "1.20.1".into(),
            loader: Some(NeoForge),
            category: None,
            page: 3,
        };
        let u = cf_search_url(&q, 20);
        // 有词走 Relevancy(13)、页 3 的 offset = 40、加载器号 6、版本原样带
        assert!(u.contains("sortField=13"), "{u}");
        assert!(u.contains("index=40"), "{u}");
        assert!(u.contains("searchFilter=sodium"), "{u}");
        assert!(u.contains("gameVersion=1.20.1"), "{u}");
        assert!(u.contains("modLoaderType=6"), "{u}");
        assert!(u.contains("gameId=432"), "{u}");
        assert!(u.contains("classId=6"), "{u}");

        // 无词按下载量排；「全部版本」时不发 modLoaderType（平台要求与 gameVersion 搭配）
        let bare = ModSearchQuery {
            text: String::new(),
            mc_version: String::new(),
            loader: Some(Fabric),
            page: 1,
            ..q
        };
        let u2 = cf_search_url(&bare, 20);
        assert!(u2.contains("sortField=2"), "{u2}");
        assert!(u2.contains("index=0"), "{u2}");
        assert!(!u2.contains("modLoaderType"), "{u2} 没有版本时不该发加载器筛选");
        assert!(!u2.contains("searchFilter"), "{u2}");
    }

    #[test]
    fn cf_hash_and_tokens() {
        let f = serde_json::json!({
            "hashes": [ {"value": "d41d8cd98f00b204e9800998ecf8427e", "algo": 1},
                        {"value": "0F1E2D3C4B5A69788796A5B4C3D2E1F0A9B8C7D6", "algo": 2} ]
        });
        // algo 号两边都说是自己那一家：只按长度认，取到 40 位那条并归一小写
        assert_eq!(
            cf_sha1(&f).as_deref(),
            Some("0f1e2d3c4b5a69788796a5b4c3d2e1f0a9b8c7d6")
        );
        assert_eq!(cf_sha1(&serde_json::json!({"hashes": []})), None);
        assert_eq!(cf_sha1(&serde_json::json!({})), None);

        // 版本 token 混着加载器名与端标签，只认长得像版本号的那个
        let v = serde_json::json!({"gameVersions": ["Java Edition", "Fabric", "1.20.4"]});
        assert_eq!(cf_mc_version(&v), "1.20.4");
        assert_eq!(cf_mc_version(&serde_json::json!({"gameVersions": ["Fabric"]})), "");
        // NeoForge 名字里含 forge，判定顺序错了就会认成 Forge
        assert_eq!(cf_loader(&serde_json::json!({"gameVersions": ["NeoForge"]})), NeoForge);
        assert_eq!(cf_loader(&serde_json::json!({"gameVersions": ["Forge"]})), Forge);
        assert_eq!(cf_loader(&serde_json::json!({"gameVersions": ["Quilt"]})), Fabric);
        assert_eq!(cf_loader(&serde_json::json!({})), Fabric, "认不出兜底 Fabric");
        assert_eq!(cf_loader_type(Forge), 1);
        assert_eq!(cf_loader_type(Fabric), 4);
        assert_eq!(cf_loader_type(NeoForge), 6);
    }

    #[test]
    fn cf_file_entry_maps_fields_and_drops_unavailable() {
        let f: Value = serde_json::from_str(&serde_json::json!({
            "id": 4001, "displayName": "YACL 1.20.1", "fileName": "yacl-1.20.1.jar",
            "isAvailable": true, "fileLength": 512000, "fileDate": "2023-11-02T10:00:00Z",
            "gameVersions": ["1.20.1", "Fabric", "Client/Server"],
            "hashes": [ {"value": "0f1e2d3c4b5a69788796a5b4c3d2e1f0a9b8c7d6", "algo": 2} ]
        })
        .to_string())
        .unwrap();
        let e = file_entry(&f, "1.20.1").expect("可用构建应映射出来");
        assert_eq!(e.id, "4001");
        assert_eq!(e.version_number, "YACL 1.20.1");
        assert_eq!(e.file_name, "yacl-1.20.1.jar");
        assert_eq!(e.mc_version, "1.20.1");
        assert_eq!(e.loader, Fabric);
        assert_eq!(e.date, "2023-11-02");
        assert_eq!(e.size_bytes, 512000);
        assert_eq!(
            e.sha1.as_deref(),
            Some("0f1e2d3c4b5a69788796a5b4c3d2e1f0a9b8c7d6")
        );
        // 临时链接不落档：构建期再按 id 现取
        assert_eq!(e.url, "");
        // CF 没有端声明这一说：宁缺毋滥，两个都留 None 让离线取证层去答
        assert_eq!(e.client_side, None);
        assert_eq!(e.server_side, None);
        assert!(!e.recommended);

        assert!(file_entry(&serde_json::from_str::<Value>(UNAVAILABLE).unwrap(), "1.20.1").is_none());
        // displayName 空 → 回落文件名，版本行不会显示成空白
        let blank: Value = serde_json::json!({"id": 7, "displayName": "  ", "fileName": "b.jar",
                                              "gameVersions": ["1.20.1"], "hashes": []});
        assert_eq!(file_entry(&blank, "").unwrap().version_number, "b.jar");
    }

    #[test]
    fn cf_search_page_maps_total_and_fields() {
        let v: Value = serde_json::from_str(FIXTURE).unwrap();
        let m = &v["data"][0];
        // 这一组断言钉的是「前端那三个栏位读哪几个 CF 字段」，改了 CF 或改了这里都会红
        assert_eq!(ident(&m["id"]), "301445", "数字 id 要转成字符串");
        assert_eq!(m["name"].as_str().unwrap(), "YetAnotherConfigLib");
        assert_eq!(m["summary"].as_str().unwrap(), "A config library made with modern frameworks");
        assert_eq!(m["authors"][0]["name"].as_str().unwrap(), "XanderIsDeviant");
        assert_eq!(num(&m["downloadCount"]), 8123456);
        assert_eq!(
            m["logo"]["thumbnailUrl"].as_str().unwrap(),
            "https://media.forgecdn.net/avatars/1/2/logo.png"
        );
        assert_eq!(num(&v["pagination"]["totalCount"]), 4321);
        // 编译期口径：这些字段在 CF 侧恒为 None / 固定值
        let r = ModSearchResult {
            slug: m["slug"].as_str().map(String::from),
            id: ident(&m["id"]),
            name: m["name"].as_str().unwrap_or_default().to_string(),
            name_zh: mcmod_names::zh_name_of(m["slug"].as_str()),
            description: m["summary"].as_str().unwrap_or_default().to_string(),
            author: m["authors"][0]["name"].as_str().unwrap_or_default().to_string(),
            downloads: num(&m["downloadCount"]),
            icon_url: m["logo"]["thumbnailUrl"].as_str().map(String::from),
            source: ModSource::Curseforge,
            compatible: true,
            already_added: false,
            client_side: None,
            server_side: None,
        };
        assert_eq!(r.client_side, None::<SideFlag>);
        // slug 是中文简介那条线的入场券（麦块 detail/{slug} 只认它，不认数字 id）
        assert_eq!(r.slug.as_deref(), Some("yacl"));
        assert_eq!(r.source, ModSource::Curseforge);
    }

    /// 真联网：CF 那几类查询在镜像这条**唯一候选**上是否还答得出来（2026-10-01：镜像会间歇回
    /// 502 或干脆挂住，同一枚 mod id 连敲两次 502、几分钟后又全程 200）。用途是把「镜像此刻不通」
    /// 与「我们自己的响应解析坏了」分开——界面只会说一句网络话，读不出这两档。
    /// 只钉「答得出来」，不钉条数/版本/链的内容（远端词表会变，直链还带时效）
    #[tokio::test]
    #[ignore = "真联网：搜索 / 类别 / 构建列表 / 前置批量反查 / 取直链各发一发"]
    async fn curseforge_chain_answers_through_the_mirror() {
        let d = Downloader::new(std::env::temp_dir(), 1);
        let q = ModSearchQuery {
            source: ModSource::Curseforge,
            text: "fabric-api".into(),
            mc_version: String::new(),
            loader: None,
            category: None,
            page: 1,
        };
        let page = d.search_curseforge(&q).await.expect("搜索这一发就挂了");
        let hit = page.results.first().expect("第一屏就是空");
        let cats = d.list_curseforge_categories().await.expect("类别这一发挂了");
        let files = d.list_curseforge_versions(&hit.id, "").await.expect("构建列表挂了");
        let file = files.first().expect("这个项目一个构建都没解析出来");
        // 前置那张批量口（POST /v1/mods）内部吞错、永远返回表，所以这里只看它挂没挂上名字
        let briefs = d.curseforge_mod_briefs(std::slice::from_ref(&hit.id)).await;
        let link = d
            .curseforge_download_url(&hit.id, &file.id)
            .await
            .expect("取直链挂了（整项目被拒发的那类 403 也算在这条里，重试前先看详情那句）");
        assert!(cats.len() > 1, "类别只回来 {} 条", cats.len());
        assert!(link.starts_with("https://"), "取回来的不像一条链：{link}");
        println!(
            "[cf-chain] mod {} · 类别 {} · 构建 {} 枚 · 前置反查 {} 条 · 链 {link}",
            hit.id,
            cats.len(),
            files.len(),
            briefs.len()
        );
    }
}
