//! Modrinth 侧：端声明的读取与映射，以及搜索 / 构建 / 项目 / 类别 / 单文件解析五类查询。

use std::collections::HashMap;
use std::path::PathBuf;

use serde_json::Value;

use crate::models::{LoaderKind, ModSearchPage, ModSearchQuery, ModSearchResult, ModSource, ModVersionEntry, SideFlag};
use super::client::Downloader;
use super::types::{DownloadError, Fetch, ItemSpec};
use super::util::{loader_cat, urlencoding};

pub const MODRINTH_API: &str = "https://api.modrinth.com/v2";

/// Modrinth 侧的端声明原始值（字段口径见 env 模块的映射表；这里只搬运不解释）
#[derive(Debug, Clone, Default)]
pub struct ModrinthEnv {
    /// 项目级：required / optional / unsupported
    pub client_side: Option<String>,
    pub server_side: Option<String>,
    /// 构建级只有这个：client_only / server_only / client_and_server /
    /// client_only_server_optional / server_only_client_optional / client_or_server_prefers_both
    pub environment: Option<String>,
    /// 供 slug 猜测校验（文件名推的 id 是否真是这个项目）
    pub slug: Option<String>,
    pub title: Option<String>,
}

impl ModrinthEnv {
    /// 两侧支持度：优先精确的 `client_side`/`server_side`（项目级、搜索结果也有），
    /// 回落构建级 `environment` 枚举。取证层与「在线添加」的端标签共用这一份映射。
    pub fn sides(&self) -> Option<(SideFlag, SideFlag)> {
        use SideFlag::{Optional, Required, Unsupported};
        let flag = |v: &str| {
            Some(match v.trim().to_lowercase().as_str() {
                "required" => Required,
                "optional" => Optional,
                "unsupported" => Unsupported,
                _ => return None,
            })
        };
        if let (Some(c), Some(s)) = (
            self.client_side.as_deref().and_then(flag),
            self.server_side.as_deref().and_then(flag),
        ) {
            return Some((c, s));
        }
        let norm = self.environment.as_deref()?.trim().to_lowercase().replace('-', "_");
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
}

impl Downloader {
    /// Modrinth 响应里的端声明字段（项目级有 client_side/server_side，构建级只有 environment）。
    /// 镜像侧（`minekuai.rs`）字段名与它逐字相同，共用这一份解析，别处不另写一套映射
    pub(crate) fn modrinth_env(v: &Value) -> ModrinthEnv {
        let s = |k: &str| v[k].as_str().map(String::from);
        ModrinthEnv {
            client_side: s("client_side"),
            server_side: s("server_side"),
            // project.environment 实测是字符串，个别接口给数组 → 取首元素
            environment: v["environment"]
                .as_array()
                .and_then(|a| a.first())
                .and_then(|e| e.as_str())
                .map(String::from)
                .or_else(|| s("environment")),
            slug: s("slug"),
            title: s("title"),
        }
    }

    /// 按文件 sha1 批量反查构建（`POST /v2/version_files`）。
    /// 实测响应为 `{ "<sha1>": {version…, environment, project_id} }`，查不到的哈希直接缺席。
    pub async fn version_env_by_sha1(
        &self,
        hashes: &[String],
    ) -> Result<HashMap<String, ModrinthEnv>, DownloadError> {
        let mut out = HashMap::new();
        if hashes.is_empty() {
            return Ok(out);
        }
        let v = self
            .post_json(
                &format!("{MODRINTH_API}/version_files"),
                &serde_json::json!({ "hashes": hashes, "algorithm": "sha1" }),
            )
            .await?;
        if let Some(obj) = v.as_object() {
            for (k, ver) in obj {
                out.insert(k.to_lowercase(), Self::modrinth_env(ver));
            }
        }
        Ok(out)
    }

    /// 项目级端声明（`GET /v2/project/{slug|id}`）；`Ok(None)` = 项目不存在（404，不是网络故障）
    pub async fn project_env(&self, key: &str) -> Result<Option<ModrinthEnv>, DownloadError> {
        let url = format!("{MODRINTH_API}/project/{}", urlencoding(key));
        match self.get_json(&url).await {
            Ok(v) => Ok(Some(Self::modrinth_env(&v))),
            Err(DownloadError::Http { status: 404, .. }) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// 按名称搜项目（`GET /v2/search`）：命中项同时带 client_side / server_side /
    /// environment（实测），是文件名被改过、slug 猜不上时唯一还能对上平台的线索。
    /// 只做取证，不做展示，故不套搜索页的筛选与分页。
    pub async fn search_env(&self, query: &str) -> Result<Vec<ModrinthEnv>, DownloadError> {
        let url = format!(
            "{MODRINTH_API}/search?query={}&limit=5&index=relevance",
            urlencoding(query.trim())
        );
        let v = self.get_json(&url).await?;
        Ok(v["hits"]
            .as_array()
            .map(|a| a.iter().map(Self::modrinth_env).collect())
            .unwrap_or_default())
    }

    pub async fn search_mods(&self, q: &ModSearchQuery) -> Result<ModSearchPage, DownloadError> {
        // 两家平台唯一的分发点在这里（命令层不再各判一次）：CurseForge 要用户自己的 API Key，
        // 缺 Key / Key 被拒都由那一侧报可读的 `Refused`
        if q.source == ModSource::Curseforge {
            return self.search_curseforge(q).await;
        }
        let page_size = 20u32;
        let offset = (q.page.saturating_sub(1)) * page_size;
        let mut facets: Vec<Vec<String>> = Vec::new();
        if !q.mc_version.is_empty() {
            facets.push(vec![format!("versions:{}", q.mc_version)]);
        }
        if let Some(l) = q.loader {
            facets.push(vec![format!("categories:{}", loader_cat(l))]);
        }
        if let Some(cat) = &q.category {
            if !cat.is_empty() && cat != "all" {
                facets.push(vec![format!("categories:{}", cat.to_lowercase())]);
            }
        }
        let index = if q.text.trim().is_empty() {
            "downloads"
        } else {
            "relevance"
        };
        let facets_json = serde_json::to_string(&facets).unwrap_or_else(|_| "[]".into());
        let url = format!(
            "{MODRINTH_API}/search?query={}&limit={page_size}&offset={offset}&index={}&facets={}",
            urlencoding(q.text.trim()),
            index,
            urlencoding(&facets_json)
        );
        let v = self.get_json(&url).await?;
        let total = v["total_hits"].as_u64().unwrap_or(0);
        let mut results = Vec::new();
        if let Some(hits) = v["hits"].as_array() {
            for h in hits {
                // 搜索命中项自带项目级两侧支持度：端标签在「添加之前」就该看得见
                let sides = Self::modrinth_env(h).sides();
                let client_side = sides.map(|(c, _)| c);
                let server_side = sides.map(|(_, s)| s);
                results.push(ModSearchResult {
                    // Modrinth 的搜索命中里 slug 就是项目标识：id 与 slug 同值，
                    // 分成两个字段是为了让「数字 id 的那家」（CurseForge）也能走 detail/{slug}
                    slug: h["slug"].as_str().map(String::from),
                    id: h["slug"].as_str().unwrap_or_default().to_string(),
                    name: h["title"].as_str().unwrap_or_default().to_string(),
                    description: h["description"].as_str().unwrap_or_default().to_string(),
                    author: h["author"].as_str().unwrap_or_default().to_string(),
                    downloads: h["downloads"].as_u64().unwrap_or(0),
                    icon_url: h["icon_url"].as_str().map(String::from),
                    source: ModSource::Modrinth,
                    compatible: true,
                    already_added: false, // 由 commands 层按当前方案回填
                    client_side,
                    server_side,
                });
            }
        }
        Ok(ModSearchPage {
            source: ModSource::Modrinth,
            total,
            results,
            page: q.page,
            page_size,
        })
    }

    /// 某模组的可用构建（按请求 MC 版本兼容性排序）。`source` 决定查哪家
    pub async fn list_mod_versions(
        &self,
        source: ModSource,
        mod_id: &str,
        mc_version: &str,
    ) -> Result<Vec<ModVersionEntry>, DownloadError> {
        if source == ModSource::Curseforge {
            return self.list_curseforge_versions(mod_id, mc_version).await;
        }
        let url = format!("{MODRINTH_API}/project/{mod_id}/version");
        let v = self.get_json(&url).await?;
        let mut entries = Vec::new();
        if let Some(arr) = v.as_array() {
            for e in arr {
                let game_versions: Vec<String> = e["game_versions"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|g| g.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                let loaders: Vec<String> = e["loaders"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|g| g.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                let Some(file) = e["files"].as_array().and_then(|f| {
                    f.iter()
                        .find(|x| x["primary"].as_bool().unwrap_or(false))
                        .or(f.first())
                }) else {
                    continue;
                };
                let mc = if game_versions.iter().any(|g| g == mc_version) {
                    mc_version.to_string()
                } else {
                    game_versions.first().cloned().unwrap_or_default()
                };
                let loader = loaders
                    .iter()
                    .find(|l| **l == "fabric" || **l == "forge" || **l == "neoforge")
                    .and_then(|l| match l.as_str() {
                        "fabric" => Some(LoaderKind::Fabric),
                        "forge" => Some(LoaderKind::Forge),
                        "neoforge" => Some(LoaderKind::NeoForge),
                        _ => None,
                    })
                    .unwrap_or(LoaderKind::Fabric);
                let sides = Self::modrinth_env(e).sides();
                entries.push(ModVersionEntry {
                    id: e["id"].as_str().unwrap_or_default().to_string(),
                    version_number: e["version_number"].as_str().unwrap_or_default().to_string(),
                    mc_version: mc,
                    loader,
                    date: e["date_published"]
                        .as_str()
                        .map(|s| s.chars().take(10).collect())
                        .unwrap_or_default(),
                    size_bytes: file["size"].as_u64().unwrap_or(0),
                    recommended: false,
                    url: file["url"].as_str().unwrap_or_default().to_string(),
                    sha1: file["hashes"]["sha1"].as_str().map(String::from),
                    file_name: file["filename"].as_str().unwrap_or("mod.jar").to_string(),
                    client_side: sides.map(|(c, _)| c),
                    server_side: sides.map(|(_, s)| s),
                });
            }
        }
        // 兼容请求 MC 版本的构建排前，组内按发布日期倒序；首个标推荐
        entries.sort_by(|a, b| {
            (a.mc_version != mc_version).cmp(&(b.mc_version != mc_version)).then(
                b.date.cmp(&a.date),
            )
        });
        if let Some(first) = entries.first_mut() {
            first.recommended = true;
        }
        Ok(entries)
    }

    /// 类别标签（Modrinth 走 GET /tag/category，CF 走免 Key 的 /categories），供「类别」下拉
    pub async fn list_mod_categories(&self, source: ModSource) -> Result<Vec<String>, DownloadError> {
        if source == ModSource::Curseforge {
            return self.list_curseforge_categories().await;
        }
        let v = self.get_json(&format!("{MODRINTH_API}/tag/category")).await?;
        let mut out: Vec<String> = Vec::new();
        if let Some(arr) = v.as_array() {
            for e in arr {
                let pt = e["project_type"].as_str().unwrap_or("");
                if pt != "mod" && pt != "all" {
                    continue;
                }
                if let Some(n) = e["name"].as_str() {
                    if !out.iter().any(|x| x == n) {
                        out.push(n.to_string());
                    }
                }
            }
        }
        out.sort();
        Ok(out)
    }

    /// 解析某模组在 (mc, loader) 下最新 release 构建的下载地址
    pub async fn resolve_mod_file(
        &self,
        mod_id: &str,
        mc_version: &str,
        loader: LoaderKind,
    ) -> Result<ItemSpec, DownloadError> {
        let url = format!(
            "{MODRINTH_API}/project/{mod_id}/version?game_versions={}&loaders={}",
            urlencoding(&format!("[\"{mc_version}\"]")),
            urlencoding(&format!("[\"{}\"]", loader_cat(loader)))
        );
        let v = self.get_json(&url).await?;
        let arr = v.as_array().ok_or_else(|| DownloadError::NotFound(mod_id.into()))?;
        for e in arr {
            if e["version_type"].as_str() != Some("release") {
                continue;
            }
            if let Some(file) = e["files"].as_array().and_then(|f| {
                f.iter()
                    .find(|x| x["primary"].as_bool().unwrap_or(false))
                    .or(f.first())
            }) {
                let Some(u) = file["url"].as_str() else { continue };
                return Ok(ItemSpec {
                    fetch: Fetch::Url(u.to_string()),
                    file_name: file["filename"].as_str().unwrap_or("mod.jar").to_string(),
                    sha1: file["hashes"]["sha1"].as_str().map(String::from),
                    dest: PathBuf::new(), // 由调用方补目标路径
                    size_bytes: file["size"].as_u64().unwrap_or(0),
                });
            }
        }
        Err(DownloadError::NotFound(format!(
            "{mod_id}（{mc_version} / {}）",
            loader.as_label()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modrinth_sides_prefers_project_flags_and_falls_back_to_environment() {
        use SideFlag::{Optional, Required, Unsupported};
        let env = |c: Option<&str>, s: Option<&str>, e: Option<&str>| ModrinthEnv {
            client_side: c.map(str::to_string),
            server_side: s.map(str::to_string),
            environment: e.map(str::to_string),
            slug: None,
            title: None,
        };
        // 项目级两侧齐全：直接用，不看构建级
        assert_eq!(
            env(Some("required"), Some("unsupported"), Some("client_and_server")).sides(),
            Some((Required, Unsupported))
        );
        // 只有一侧（大小写/空白都得吃下）：信息不完整，退回构建级
        assert_eq!(
            env(Some("Required"), None, Some("Server-Only")).sides(),
            Some((Unsupported, Required))
        );
        // 构建级六个取值全覆盖
        for (e, want) in [
            ("client_only", (Required, Unsupported)),
            ("client_only_server_optional", (Required, Optional)),
            ("server_only", (Unsupported, Required)),
            ("server_only_client_optional", (Optional, Required)),
            ("client_and_server", (Required, Required)),
            ("client_or_server", (Optional, Optional)),
            ("client_or_server_prefers_both", (Optional, Optional)),
        ] {
            assert_eq!(env(None, None, Some(e)).sides(), Some(want), "{e}");
        }
        // 认不出来的新枚举值：宁缺毋滥
        assert_eq!(env(None, None, Some("some_new_value")).sides(), None);
        assert_eq!(env(None, None, None).sides(), None);
    }
}
