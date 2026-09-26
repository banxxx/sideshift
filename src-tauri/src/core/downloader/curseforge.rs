//! CurseForge 侧：Core API v1 的搜索 / 构建列表 / 类别 / 临时下载链四类查询。
//!
//! **与 Modrinth 的三处口径差异**（都体现在返回字段里，不在这里解释第二遍）：
//! 1. 每条请求都要 `x-api-key`（用户自己在 CurseForge 官方表单申请，存 settings.json）；
//!    唯一例外是 `/categories`——实测带 `classId` 参数会被拒（403），
//!    不带则**免 Key 200**，所以类别下拉一次 Key 也不消耗。
//! 2. **没有端声明**：CF 的 file 对象里没有任何 client/server 字段，
//!    所以搜索与构建列表的 `client_side`/`server_side` 一律 None，
//!    端判定只能等 jar 到手走离线取证层。宁缺毋滥，不从 `gameVersions` 的标签猜。
//! 3. **下载链是临时的**：`latestFiles[].downloadUrl` 可空且带时效，所以这里给出去的
//!    `url` 恒为空串，构建时再按 (mod id, file id) 现取一条（见 `curseforge_download_url`）。
//!
//! 枚举值口径抄自官方 OpenAPI（`/v1/mods/search` 的 `modLoaderType`、file 的 `hashes[].algo`）；
//! 但 `algo` 的 1/2 在各语言实现里公认对不上（文档写 1=Sha1、2=Md5，多个第三方库相反），
//! 所以校验值按**长度认**（40=sha1、32=md5），不信那个枚举号。

use serde_json::Value;

use crate::models::{
    LoaderKind, ModSearchPage, ModSearchQuery, ModSearchResult, ModSource, ModVersionEntry,
};
use super::client::Downloader;
use super::types::DownloadError;
use super::util::urlencoding;

const CURSEFORGE_API: &str = "https://api.curseforge.com/v1";

/// Minecraft 在 CurseForge 的 game id
const MC_GAME_ID: u32 = 432;
/// `classId` = Mods。同一个 game 下还混着整合包/材质包/世界/插件，不带这一条会搜出一堆 modpack
const CLASS_MODS: u32 = 6;

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
        // CF 无端声明：留给离线取证层
        client_side: None,
        server_side: None,
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
        let v = self.cf_get_json(&url).await?;
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
        let v = self.cf_get_json(&url).await?;
        let mut entries: Vec<ModVersionEntry> = v["data"]
            .as_array()
            .map(|a| a.iter().filter_map(|f| file_entry(f, mc_version)).collect())
            .unwrap_or_default();
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

    /// 现取一条构建的下载直链。返回的 URL 带时效，所以只在构建期调用、不落档不缓存。
    /// 响应按官方文档是 `{data: "<url>"}`；个别版本给 `{data:{downloadUrl}}` → 两种都认
    pub async fn curseforge_download_url(
        &self,
        mod_id: &str,
        file_id: &str,
    ) -> Result<String, DownloadError> {
        let url = format!("{CURSEFORGE_API}/mods/{mod_id}/files/{file_id}/download-url");
        let v = self.cf_get_json(&url).await?;
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::SideFlag;
    use LoaderKind::*;

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
}
