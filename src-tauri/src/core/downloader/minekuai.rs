//! 麦块开放 API（`api.minekuai.cn`）：Modrinth 项目目录的**国内只读镜像**，只用来给端判定加速。
//!
//! **它是什么**：第三方维护的 Modrinth 项目快照（附带机器翻译的中文标题）。2026-09-26 本机实测：
//! - 单次查询 0.15~0.27s，同一台机器打官方 `api.modrinth.com` 是 2.0~2.4s ⇒ 约快 10 倍；
//! - 抽样的五个项目 `client_side`/`server_side` 与官方**逐条一致**；
//! - **只认 slug**（`detail/u6dRKJwZ` 这种 project_id 形式实测 404）⇒ URL 里带出的权威
//!   project_id 那条候选仍只能走官方；
//! - **没有**按 sha1 反查构建的端点 ⇒ 第 3 层（精确到这一个文件的 `environment`）它答不上；
//! - 收录不全（抽样的 `coppered-equipment` 不在册，返回 404）⇒ 选这一档时不在册的模组就是没有结论。
//!
//! 所以这一层只接第 4 层（项目级 + 按名搜索）两件事。
//!
//! **它现在是「端信息反查源」那一档选中的源，不是加速件**：设置里选麦块 ⇒ 联网那一轮只发这里，
//! 官方三条腿（含它没有对应端点的 sha1 批量那条）一条都不发；选官方 ⇒ 这里零流量。
//! 所以它挂了那一轮就是没跑完（`resolve_online` 报 `complete=false`），不再悄悄回落官方。
//!
//! CF 那半边（`mc-cf-mods`）实测 `client_side`/`server_side` **恒为空串**（CurseForge 平台本身
//! 就没有端声明字段），取证拿不到东西，所以一个函数都不接——就算接了，`ModrinthEnv::sides`
//! 对空串本来也返回 `None`，不会污染裁决，但白搭一条腿没有意义。
//!
//! 另有一个坑：这个服务的「路径不存在」是 **200** 回来的（`{"code":404,"msg":"No endpoint …"}`），
//! 只比状态码会被骗 ⇒ 存活自查必须验字段。

use super::client::Downloader;
use super::modrinth::ModrinthEnv;
use super::types::DownloadError;
use super::util::urlencoding;
use crate::models::{ModSource, ModTranslation};
use serde_json::Value;

pub const MINEKUAI_API: &str = "https://api.minekuai.cn/panel/public";

/// 一轮开始前对镜像两条入口各发一发的体检结果。分开记两档 ⇒ 对方只改其中一条路径时，
/// 另一条不会被连带判死
pub struct MirrorHealth {
    /// `mc-mods/detail/{slug}` 还能用
    pub project: bool,
    /// `mc-mods?q=` 还能用
    pub search: bool,
}

impl Downloader {
    /// 存活自查：`env::resolve_via_mirror` 在派任何镜像腿之前先跑这一次，两条入口各一请求。
    /// 刻意不重试、不猜：任一入口验不过 ⇒ 那一腿本轮整批不派；这一档没有官方链可回落，
    /// 所以验不过直接记成「这一轮没跑完」。
    pub(crate) async fn mirror_health(&self) -> MirrorHealth {
        // 详情：认一个必然在册的项目，而且必须看到 `client_side` 是字符串——
        // 路径被改名时它回的是 200 + `{"code":404,…}`，只看状态码会当成活着
        let project = matches!(
            self.get_json(&format!("{MINEKUAI_API}/mc-mods/detail/sodium"))
                .await,
            Ok(v) if v["slug"].as_str() == Some("sodium") && v["client_side"].is_string()
        );
        // 搜索：故意查一个不存在的词，只要求拿到 `items` 数组（响应 44 字节，不为此多拉一屏数据）
        let search = matches!(
            self.get_json(&format!("{MINEKUAI_API}/mc-mods?q=zzqqxx&limit=1"))
                .await,
            Ok(v) if v["items"].is_array()
        );
        MirrorHealth { project, search }
    }

    /// 项目级端声明（`GET /mc-mods/detail/{slug}`）。`Ok(None)` = 不在收录范围（真 404），
    /// 与官方 `project_env` 同一口径：404 不是故障，换下一条腿
    pub(crate) async fn project_env_via_mirror(
        &self,
        slug: &str,
    ) -> Result<Option<ModrinthEnv>, DownloadError> {
        let url = format!("{MINEKUAI_API}/mc-mods/detail/{}", urlencoding(slug));
        match self.get_json(&url).await {
            Ok(v) => {
                let m = Self::modrinth_env(&v);
                // 路径不存在时那份 200 + `{"code":404}` 会被解析成一个全空的 ModrinthEnv：
                // 没有 slug 就等于没查到，别让它在本地索引里挂出一条空记录
                if m.slug.is_none() {
                    return Ok(None);
                }
                Ok(Some(m))
            }
            Err(DownloadError::Http { status: 404, .. }) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// 按名搜索（`GET /mc-mods?q=<词>`）：列表条目自带 `client_side`/`server_side`，
    /// 一次请求顶官方两发。查询词参数是 `q`（实测 `query=`/`keyword=` 会被忽略、返回默认列表）。
    /// 中文词查得动（2026-09-26 复测：`q=小地图` 命中 Entity Minimap，`q=优化` 命中 Cloud Tweaks，
    /// 靠的是收录里的 `title_zh`），但端判定这条线**不按中文名反查**——它认的是英文项目名/slug，
    /// 换中文词只会把「按英文名找」那件事做歪，所以这里只是把官方那 2.2s 换成 0.2s
    pub(crate) async fn search_env_via_mirror(
        &self,
        query: &str,
    ) -> Result<Vec<ModrinthEnv>, DownloadError> {
        let url = format!(
            "{MINEKUAI_API}/mc-mods?q={}&limit=5",
            urlencoding(query.trim())
        );
        let v = self.get_json(&url).await?;
        Ok(v["items"]
            .as_array()
            .map(|a| a.iter().map(Self::modrinth_env).collect())
            .unwrap_or_default())
    }

    /// 中文译文（`{入口}/detail/{slug}` 的 `title_zh` + `description_zh`）：用户在详情页点「翻译」
    /// 时才发这一发，与端判定的阶梯/预算无关，所以不查 `mirror_health`——挂了由调用方演成一次失败提示即可。
    /// **两条入口都只认 slug**（`mc-mods/detail/AANobbMI`、`mc-cf-mods/detail/32274` 实测 404），
    /// 而 CurseForge 那边我们手上的数字 id 不是 slug ⇒ 搜索结果必须带 slug 才有这条线。
    /// `Ok(None)` = 镜像没有这句译文：不在收录（真 404）、路径被改名（200 + `{"code":404}`）、
    /// 或长尾 `translation_status=pending` 时两个字段都是**空串**（实测）。三种都让前端保持原文，
    /// 绝不演成「翻译成功但内容与原文一样」
    pub(crate) async fn translate_zh(
        &self,
        source: ModSource,
        slug: &str,
    ) -> Result<Option<ModTranslation>, DownloadError> {
        let slug = slug.trim();
        if slug.is_empty() {
            return Ok(None);
        }
        let entry = match source {
            ModSource::Modrinth => "mc-mods",
            ModSource::Curseforge => "mc-cf-mods",
        };
        let url = format!("{MINEKUAI_API}/{}/detail/{}", entry, urlencoding(slug));
        match self.get_json(&url).await {
            Ok(v) => Ok(zh_fields(&v)),
            Err(DownloadError::Http { status: 404, .. }) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

/// 从 `detail/{slug}` 的响应里取那两份中文：空串与缺字段同口径（都没有译文）。
/// 名与简介各自可缺（`title_zh` 覆盖率明显低于 `description_zh`），两个都空才整体答 None
fn zh_fields(v: &Value) -> Option<ModTranslation> {
    let field = |k: &str| {
        v[k]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
    };
    let (title_zh, description_zh) = (field("title_zh"), field("description_zh"));
    (title_zh.is_some() || description_zh.is_some())
        .then_some(ModTranslation { title_zh, description_zh })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 这个服务的「路径不存在」回的是 200 + `{"code":404,…}`：那种响应必须被当成没查到，
    /// 而不是一个 slug 为 None 之外的什么东西（否则会在本地索引里挂一条空结论）
    #[test]
    fn empty_mirror_rows_carry_no_slug() {
        let v = serde_json::json!({"code": 404, "msg": "No endpoint GET /public/mc-mods/zzz."});
        let m = ModrinthEnv {
            slug: v["slug"].as_str().map(String::from),
            ..Default::default()
        };
        assert!(m.slug.is_none());
        assert!(m.sides().is_none());
    }

    /// CF 那半边实测 env 是空串：`sides()` 必须答 None（接了也不会污染裁决）
    #[test]
    fn blank_side_strings_are_not_evidence() {
        let m = ModrinthEnv {
            client_side: Some(String::new()),
            server_side: Some(String::new()),
            ..Default::default()
        };
        assert!(m.sides().is_none());
    }

    /// 译文没落地时收录里是**空串**（长尾 `translation_status=pending`），路径被改名时回的是
    /// 200 + `{"code":404}`：两种都得答 None 让调用方保持原文。演成「翻译成功但内容与原文一致」
    /// 比不翻更糟——用户会以为那句中文就是作者原文。另外名与简介各自可缺（`title_zh` 覆盖率低），
    /// 只拿到其中一个也算有译文
    #[test]
    fn missing_or_blank_zh_fields_are_not_a_translation() {
        let pending = serde_json::json!({"slug": "x", "title_zh": "", "description_zh": ""});
        assert!(zh_fields(&pending).is_none());
        let renamed = serde_json::json!({"code": 404, "msg": "No endpoint GET /public/mc-mods/detail/zzz."});
        assert!(zh_fields(&renamed).is_none());
        // 实测形状：简介翻好了、名字还没翻（sodium/krypton 都是这一档）
        let only_desc = serde_json::json!({"title_zh": "", "description_zh": "  一个模组。 "});
        let got = zh_fields(&only_desc).expect("有简介就算有译文");
        assert_eq!(got.title_zh, None);
        assert_eq!(got.description_zh.as_deref(), Some("一个模组。"));
        let only_title = serde_json::json!({"title_zh": "实体小地图", "description_zh": ""});
        let got = zh_fields(&only_title).expect("有名就算有译文");
        assert_eq!(got.title_zh.as_deref(), Some("实体小地图"));
        assert_eq!(got.description_zh, None);
    }
}
