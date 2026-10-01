//! 麦块开放 API（`api.minekuai.cn`）：Modrinth 项目目录的国内只读镜像，端信息反查档选中的源而非加速件——
//! 选它时联网一轮只发这里，失败即本轮没跑完（`resolve_online` 报 `complete=false`），不悄悄回落官方。
//! 能力边界：只认 slug（project_id 形式 404）；无 sha1 反查端点，只接项目级与按名搜索两类查询；收录不全；CF 半边端字段恒为空串，一概不接。
//! 注意：该服务「路径不存在」以 200 + `{"code":404,…}` 返回，存活自查必须验字段。

use super::client::Downloader;
use super::mcmod::norm_name;
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
    ///
    /// **CF 半边缺的译文去 Modrinth 半边借**（`name` 是调用方带来的显示名，作验同形的备用锚）。
    /// 两个快照的翻译覆盖是两本账：2026-10 实测 jei / appleskin 在 CF 半边 `title_zh` 是空串，
    /// 同一 slug 在 Modrinth 半边有现成中文——只查来源那半边，用户就会看到「API 明明有中文，
    /// 界面翻不出来」。借之前拿两边的英文 title 归一化验同形（`same_mod`：全等或前缀），
    /// 对不上宁可不借——绝不把别的模组的译名挂过来。Modrinth 来源不借：它就是译文覆盖更好的
    /// 那半边，CF 半边只会更空。
    /// `Ok(None)` = 镜像没有译文：不在收录（真 404）、路径被改名（200 + `{"code":404}`）、
    /// 或长尾 `translation_status=pending` 时两个字段都是**空串**（实测）。这些情况都让前端
    /// 保持原文，绝不演成「翻译成功但内容与原文一样」
    pub(crate) async fn translate_zh(
        &self,
        source: ModSource,
        slug: &str,
        name: Option<&str>,
    ) -> Result<Option<ModTranslation>, DownloadError> {
        let slug = slug.trim();
        if slug.is_empty() {
            return Ok(None);
        }
        let entry = match source {
            ModSource::Modrinth => "mc-mods",
            ModSource::Curseforge => "mc-cf-mods",
        };
        let own_url = format!("{MINEKUAI_API}/{}/detail/{}", entry, urlencoding(slug));
        let own = match self.get_json(&own_url).await {
            Ok(v) => Some(v),
            // 真 404 = 不在收录，不是故障：CF 来源还有 Modrinth 半边可借
            Err(DownloadError::Http { status: 404, .. }) => None,
            Err(e) => return Err(e),
        };
        // 验同形的锚：响应自带的英文 title 优先（它就是这本在来源平台的名字），
        // 响应没有（不在收录）退调用方带来的显示名
        let anchor = own
            .as_ref()
            .and_then(|v| v["title"].as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .or_else(|| name.map(str::trim).filter(|n| !n.is_empty()).map(String::from));
        let mut tr = own.as_ref().and_then(|v| zh_fields(v));
        if source == ModSource::Curseforge
            && tr
                .as_ref()
                .is_none_or(|t| t.title_zh.is_none() || t.description_zh.is_none())
        {
            if let Some(anchor) = anchor {
                let alt_url = format!("{MINEKUAI_API}/mc-mods/detail/{}", urlencoding(slug));
                if let Ok(alt) = self.get_json(&alt_url).await {
                    if same_mod(&anchor, alt["title"].as_str().unwrap_or_default()) {
                        fill_missing(&mut tr, zh_fields(&alt));
                    }
                }
            }
        }
        Ok(tr)
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

/// 两个平台的英文 title 是否同一个模组：归一化（剥所有格 + 只留字母数字小写，
/// 与百科腿的 `mcmod::norm_name` 同一份）后**全等**，或**短侧是长侧的前缀且短侧
/// 至少 5 个字符**——后者吃下 "Cloth Config API" ↔ "Cloth Config API (Fabric/Forge/
/// NeoForge)" 这种一侧带平台注记的写法；5 个字符的门槛挡住 "JEI" ↔ "JEI (Legacy)"
/// 这类短名衍生分支。对不上宁可不借：把别的模组的译名挂过来比不翻更糟
fn same_mod(a: &str, b: &str) -> bool {
    let (a, b) = (norm_name(a), norm_name(b));
    if a.is_empty() || b.is_empty() {
        return false;
    }
    a == b
        || ((a.starts_with(&b) || b.starts_with(&a)) && a.len().min(b.len()) >= 5)
}

/// 把借来的译文填进缺的那几格：两侧都有的以来源侧为准（它才是用户所查平台自己的档案）
fn fill_missing(base: &mut Option<ModTranslation>, alt: Option<ModTranslation>) {
    match (base.as_mut(), alt) {
        (Some(b), Some(a)) => {
            if b.title_zh.is_none() {
                b.title_zh = a.title_zh;
            }
            if b.description_zh.is_none() {
                b.description_zh = a.description_zh;
            }
        }
        (None, a) => *base = a,
        _ => {}
    }
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

    /// 同形闸：全等（含大小写与平台注记差异）或「短侧前缀且 ≥5 字符」。
    /// 门槛挡的是 "JEI" ↔ "JEI (Legacy)" 这种短名衍生分支——借错译名比不翻更糟
    #[test]
    fn same_mod_accepts_equal_or_long_prefix_only() {
        assert!(same_mod("Just Enough Items (JEI)", "Just Enough Items (JEI)"));
        // 大小写归一
        assert!(same_mod("AppleSkin", "appleskin"));
        // 前缀：一侧带平台注记（实测 cloth-config 的两半边就长这样）
        assert!(same_mod(
            "Cloth Config API",
            "Cloth Config API (Fabric/Forge/NeoForge)"
        ));
        // 所有格剥掉后同形
        assert!(same_mod("Farmer's Delight", "Farmer's Delight"));
        // 短名衍生分支：前缀成立但短侧只有 3 个字符 ⇒ 拒
        assert!(!same_mod("JEI", "JEI (Legacy)"));
        // 完全不同的两个模组
        assert!(!same_mod("Sodium", "Just Enough Items (JEI)"));
        // 空锚（响应没 title 也没显示名）不借
        assert!(!same_mod("", "Sodium"));
    }

    /// 借译文只填缺的格：来源侧已有的结论是权威，借来的不许顶掉；
    /// 来源侧整本没译文时整本接住
    #[test]
    fn fill_missing_takes_only_the_gaps() {
        let mut base = Some(ModTranslation {
            title_zh: None,
            description_zh: Some("来源侧的简介".into()),
        });
        let alt = Some(ModTranslation {
            title_zh: Some("借来的名字".into()),
            description_zh: Some("借来的简介".into()),
        });
        fill_missing(&mut base, alt);
        let b = base.expect("有借必有出");
        assert_eq!(b.title_zh.as_deref(), Some("借来的名字"), "缺的格被填上");
        assert_eq!(
            b.description_zh.as_deref(),
            Some("来源侧的简介"),
            "来源侧已有的不许被借来的顶掉"
        );
        // 来源侧整本没有：整本接住
        let mut none = None;
        fill_missing(&mut none, Some(ModTranslation { title_zh: Some("x".into()), description_zh: None }));
        assert_eq!(none.and_then(|t| t.title_zh).as_deref(), Some("x"));
        // 两边都没有：仍是 None
        let mut still = None;
        fill_missing(&mut still, None);
        assert!(still.is_none());
    }
}
