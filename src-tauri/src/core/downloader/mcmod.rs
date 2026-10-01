//! MC百科（`mcmod.cn`）词条的「运行环境」字段：平台各腿全答不上时那条补全腿取的数。
//! 没有 JSON API，只有两页 HTML：`search.mcmod.cn/s?key=<词>` 的结果列表与 `www.mcmod.cn/class/{id}.html`
//! 里 `<li class="col-lg-4">运行环境: 客户端需装, 服务端无效</li>` 那一行。
//!
//! 2026-09-30 探针量出来的四条事实（决定了下面的写法）：
//! - 正文一律 UTF-8，`Content-Type` 也标 UTF-8，不需要转码；
//! - 人机验证/错参数**不回 4xx**，回的是 200 + 一段 `Jump('/')` 跳首页的脚本（百来字节）⇒ 只看状态码会把「没答」读成「没有」；
//! - 24 路并发里约六分之一连接被直接掐断（`000`），串行 16 发全过 ⇒ 这条腿的并发由调用方压到 2，别按 Modrinth 那档的宽度铺；
//! - 中文改名词条把英文名挂在标题的括号里（`[FTBQ] FTB 任务 (FTBQuests)`），那是唯一能和 jar 自报名对上的部分。

use super::client::Downloader;
use super::types::DownloadError;
use super::util::urlencoding;
use crate::models::SideFlag;

pub const MCMOD_SEARCH: &str = "https://search.mcmod.cn/s";
pub const MCMOD_CLASS: &str = "https://www.mcmod.cn/class";

/// 词条名里那段中文标签的开头（`运行环境: 客户端需装, 服务端需装`）
const ENV_LABEL: &str = "运行环境";

/// 搜索结果的一条词条
pub(crate) struct McmodHit {
    /// 词条页数字编号（`/class/{id}.html`）
    pub id: String,
    /// 列表里显示的名字（已去标签，形如 `[FTBQ] FTB 任务 (FTBQuests)`）
    pub name: String,
}

/// 词条页取到的东西：显示名 + 两侧支持度（两个字段各自可缺，缺了由调用方决定采不采信）
pub(crate) struct McmodEntry {
    pub name: String,
    pub client: Option<SideFlag>,
    pub server: Option<SideFlag>,
}

/// 一次页面请求的三种回答。**必须把「被拦」和「没有」分开**：
/// 前者要继续敲門只会加深拦截，后者是问到了、对方不收录
pub(crate) enum McmodPage<T> {
    /// 页面正常，取到了要的东西
    Answered(T),
    /// 页面正常，但没有这一条（搜不到 / 词条没写运行环境）：不算故障
    Absent,
    /// 200 + 跳首页那段脚本：这一轮别再敲了
    Blocked,
}

impl Downloader {
    /// 按名搜词条（`GET {MCMOD_SEARCH}?key=<词>`）。参数名实测是 `key`；
    /// `q=`/`query=` 那些写法会撞上那段跳转脚本，别拿去当「它不收录」的证据
    pub(crate) async fn mcmod_search(
        &self,
        query: &str,
    ) -> Result<McmodPage<Vec<McmodHit>>, DownloadError> {
        let q = query.trim();
        if q.is_empty() {
            return Ok(McmodPage::Absent);
        }
        let url = format!("{MCMOD_SEARCH}?key={}", urlencoding(q));
        match self.mcmod_html(&url).await? {
            McmodPage::Blocked => Ok(McmodPage::Blocked),
            McmodPage::Absent => Ok(McmodPage::Absent),
            McmodPage::Answered(html) => {
                let hits = search_hits(&html);
                if hits.is_empty() {
                    Ok(McmodPage::Absent)
                } else {
                    Ok(McmodPage::Answered(hits))
                }
            }
        }
    }

    /// 词条页（`GET {MCMOD_CLASS}/{id}.html`）：取显示名与「运行环境」那一行。
    /// 词条没写那一行（非模组词条、或字段被改版）算 `Absent`，不算故障
    pub(crate) async fn mcmod_entry(
        &self,
        id: &str,
    ) -> Result<McmodPage<McmodEntry>, DownloadError> {
        let url = format!("{MCMOD_CLASS}/{}.html", urlencoding(id));
        match self.mcmod_html(&url).await? {
            McmodPage::Blocked => Ok(McmodPage::Blocked),
            McmodPage::Absent => Ok(McmodPage::Absent),
            McmodPage::Answered(html) => {
                let Some((client, server)) = env_sides(&html) else {
                    return Ok(McmodPage::Absent);
                };
                // 名字是第二道闸（调用方拿它复核「这页就是我要的那个模组」）；
                // 少了 `<title>` 也照样给结论，由调用方按「只有列表名可对上」的口径决定
                Ok(McmodPage::Answered(McmodEntry {
                    name: page_title(&html).unwrap_or_default(),
                    client: Some(client),
                    server: Some(server),
                }))
            }
        }
    }

    /// 一条 HTML 原语：**不走 `get_json` 的镜像候选链**（`source` 那张表里没有 mcmod 的镜像，
    /// 换了主机名只会拿到一个陌生站点的 404），也刻意不重试——这条腿是补全，一次抖动不值得再敲一次
    async fn mcmod_html(&self, url: &str) -> Result<McmodPage<String>, DownloadError> {
        let resp = self
            .client
            .get(url)
            .timeout(super::client::METADATA_TIMEOUT)
            .send()
            .await
            .map_err(|e| super::types::net_err(url, &e))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(DownloadError::Http {
                url: url.to_string(),
                status: status.as_u16(),
            });
        }
        let text = resp
            .text()
            .await
            .map_err(|e| super::types::net_err(url, &e))?;
        // 正常结果页里从来没有 `Jump(`（实测两份真页面各 0 次），出现即人机验证出口
        if text.contains("Jump(") {
            return Ok(McmodPage::Blocked);
        }
        Ok(McmodPage::Answered(text))
    }
}

/// 从结果页切出词条列表：只认 `href="https://www.mcmod.cn/class/{数字}.html"` 的链接。
/// 同一编号在页里出现两次（标题一次、条目底部的「地址」一次），保留**带名字的那一次**；
/// 「地址」那条的锚文本就是 URL 本身，按主机名滤掉
fn search_hits(html: &str) -> Vec<McmodHit> {
    const PREFIX: &str = "href=\"https://www.mcmod.cn/class/";
    let mut out: Vec<McmodHit> = Vec::new();
    let mut at = 0usize;
    while let Some(k) = html[at..].find(PREFIX) {
        let start = at + k + PREFIX.len();
        at = start;
        let rest = &html[start..];
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() || !rest[digits.len()..].starts_with(".html\"") {
            continue;
        }
        let after = &rest[digits.len() + ".html\"".len()..];
        // `.html"` 后面紧跟 `>`（无多余属性）或还有属性再 `>`：跳过开标签剩下的部分
        let body = if let Some(tail) = after.strip_prefix('>') {
            tail
        } else {
            match after.find('>') {
                Some(p) => &after[p + 1..],
                None => continue,
            }
        };
        let Some(end) = body.find("</a>") else { continue };
        let name = decode_entities(&strip_tags(&body[..end]));
        let name = name.trim();
        if name.is_empty() || name.contains("mcmod.cn") {
            continue;
        }
        if out.iter().any(|h| h.id == digits) {
            continue;
        }
        out.push(McmodHit { id: digits, name: name.to_string() });
    }
    out
}

/// `<title>[FTBQ]FTB 任务 (FTB Quests) - MC百科|最大的Minecraft中文MOD百科</title>` → 去掉站点后缀
fn page_title(html: &str) -> Option<String> {
    let start = html.find("<title>")? + "<title>".len();
    let rest = &html[start..];
    let end = rest.find("</title>")?;
    let raw = decode_entities(&rest[..end]);
    // 后缀前有分隔符，两端各站点写法不一（` - ` / `-`），按主机名切开后把尾巴的空白与横线剃掉
    let name = raw
        .split("MC百科")
        .next()
        .unwrap_or(raw.as_str())
        .trim_end_matches(|c: char| c == '-' || c == '—' || c.is_whitespace())
        .trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// 那一行 `<li class="col-lg-4">运行环境: 客户端需装, 服务端无效</li>` → 两侧支持度。
/// **两侧齐了才算答上**：只提到一端时另一端是「百科没说」，猜成可选/无效都会把处置定歪。
/// 逐处试而不是只取第一处：页面上「运行环境」这四个字也可能出现在筛选栏/说明文字里，
/// 拿那一处去解析会得到 `None`，于是整条腿对这一行白跑
fn env_sides(html: &str) -> Option<(SideFlag, SideFlag)> {
    html.match_indices(ENV_LABEL).find_map(|(i, _)| {
        let seg = &html[i + ENV_LABEL.len()..];
        let seg = match seg.find('<') {
            Some(end) => &seg[..end],
            None => seg,
        };
        // 冒号有半角与全角两种写法（页面是编辑手填的，别赌其中一种）
        let tail = seg
            .find(':')
            .or_else(|| seg.find('：'))
            .map(|p| &seg[p + ':'.len_utf8()..])?;
        let mut client = None;
        let mut server = None;
        for clause in tail.split([',', '，']) {
            let clause = clause.trim();
            let flag = if clause.ends_with("需装") {
                SideFlag::Required
            } else if clause.ends_with("可选") {
                SideFlag::Optional
            } else if clause.ends_with("无效") {
                SideFlag::Unsupported
            } else {
                continue;
            };
            if clause.contains("客户端") {
                client = Some(flag);
            } else if clause.contains("服务端") {
                server = Some(flag);
            }
        }
        match (client, server) {
            (Some(c), Some(s)) => Some((c, s)),
            _ => None,
        }
    })
}

/// 去掉 HTML 标签（结果名里嵌着 `<em>` 高亮）。同时把实体还原一次，比对名字要用可读文本
fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth = 0usize;
    for c in s.chars() {
        match c {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

fn decode_entities(s: &str) -> String {
    s.replace("&nbsp;", " ")
        .replace("&#39;", "'")
        .replace("&#039;", "'")
        .replace("&apos;", "'")
        .replace("&quot;", "\"")
        .replace("&amp;", "&")
}

/// 名字归一化：先剥英文所有格 `'s`（`Xaero's WorldMap` ↔ 查询 `xaero-world-map` 差的就是这一个 `s`），
/// 再只留字母数字并小写（含中日韩：非字母数字的分隔符、括号、下划线一律不算差异）
pub(crate) fn norm_name(s: &str) -> String {
    let no_possessive = s
        .replace("'s", "")
        .replace("\u{2019}s", "")
        .to_lowercase();
    no_possessive
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

/// 列表名能对上查询词的几种写法：整串、去掉开头的 `[缩写]` 组、最后一对括号里的英文名
/// （词条名实测是 `[FTBQ] FTB 任务 (FTBQuests)` 这种三段式，英文名只在最后那一段）。
/// **只做等形比对、不做前缀/包含，也不取「括号前那一段」**：衍生分支就叫 `JustEnoughItems (Legacy)`，
/// 括号前那段与本体逐字相同，取它等于把分支的端声明安到本体头上；搜出来的同名衍生模组
/// （`FTBQuests Optimizer`）同理，模糊匹配会把别的模组的声明当成这一枚的依据，那正是「误判」里最糟的一种
pub(crate) fn name_forms(display: &str) -> Vec<String> {
    let mut forms = vec![norm_name(display)];
    let bare = match display.find(']') {
        Some(p) if display.starts_with('[') => display[p + 1..].trim_start(),
        _ => display,
    };
    if bare != display {
        forms.push(norm_name(bare));
    }
    for (open, close) in [('(', ')'), ('（', '）')] {
        if let (Some(a), Some(b)) = (bare.rfind(open), bare.rfind(close)) {
            if a < b {
                forms.push(norm_name(&bare[a + open.len_utf8()..b]));
            }
        }
    }
    forms.retain(|f| !f.is_empty());
    forms
}

/// 词条名 ↔ 查询候选（jar 自报显示名 + 文件名/slug 候选）是否同一个模组
pub(crate) fn mcmod_confident(cands: &[String], display: &str) -> bool {
    let want: Vec<String> = cands
        .iter()
        .map(|c| norm_name(c))
        .filter(|c| !c.is_empty())
        .collect();
    let forms = name_forms(display);
    want.iter().any(|w| forms.contains(w))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 真页面切形（2026-09-30 抓取）：结果名嵌 `<em>`，同一条链接在「地址」里重复一次且锚文本就是 URL
    const SEARCH: &str = r#"<div class="search-result-list"><div class="result-item"><div class="head"><div class="class-category"><ul><li><a class="c_24" href="//www.mcmod.cn/class/category/24-1.html" target="_blank"></a></li></ul></div><a  target="_blank" href="https://www.mcmod.cn/class/1423.html">[<em>FTB</em>Q] <em>FTB</em> 任务 (<em>FTB</em><em>Quests</em>)</a></div><div class="foot"><span class="info"><span>地址：</span><span class="value"><a  target="_blank" href="https://www.mcmod.cn/class/1423.html">www.mcmod.cn/class/1423.html</a></span></span></div></div>"#;

    #[test]
    fn search_list_keeps_named_rows_only_and_dedups() {
        let hits = search_hits(SEARCH);
        assert_eq!(hits.len(), 1, "同一条目的两次链接只留带名字那次");
        assert_eq!(hits[0].id, "1423");
        assert_eq!(hits[0].name, "[FTBQ] FTB 任务 (FTBQuests)");
    }

    #[test]
    fn env_line_maps_the_three_terms() {
        let html = r#"<li class="col-lg-4">运行环境: 客户端需装, 服务端无效</li>"#;
        assert_eq!(
            env_sides(html),
            Some((SideFlag::Required, SideFlag::Unsupported))
        );
        let reversed = r#"<li class="col-lg-4">运行环境: 客户端可选，服务端需装</li>"#;
        assert_eq!(
            env_sides(reversed),
            Some((SideFlag::Optional, SideFlag::Required))
        );
        // 只说了一端：另一端是「百科没说」，不能替它编一个值出来
        let half = r#"<li class="col-lg-4">运行环境: 客户端需装</li>"#;
        assert_eq!(env_sides(half), None);
        assert_eq!(env_sides("<li>没有那一行</li>"), None);
        // 同页别处先出现「运行环境」四个字（筛选栏/说明文字）时不能就此收队，要接着往下找
        let decoy = r#"<a href="/modlist.html?env=1">运行环境</a><li class="col-lg-4">运行环境: 客户端需装, 服务端需装</li>"#;
        assert_eq!(env_sides(decoy), Some((SideFlag::Required, SideFlag::Required)));
    }

    #[test]
    fn class_title_loses_the_site_suffix() {
        let html = "<title>钠 (Sodium) - MC百科|最大的Minecraft中文MOD百科</title>";
        assert_eq!(page_title(html).as_deref(), Some("钠 (Sodium)"));
    }

    /// 采信口径的用例：英文名只在括号里，衍生模组必须对不上（那枚是完全不同的另一个 jar）
    #[test]
    fn only_exact_name_forms_are_accepted() {
        assert!(mcmod_confident(
            &["FTB Quests".into(), "ftb-quests".into()],
            "[FTBQ] FTB 任务 (FTBQuests)"
        ));
        assert!(mcmod_confident(&["sodium".into()], "钠 (Sodium)"));
        assert!(mcmod_confident(
            &["just-enough-items".into()],
            "[JEI] JEI物品管理器 (JustEnoughItems)"
        ));
        // 英文所有格差一个 s：剥掉才对得上（页面两处写法本身还差一个空格，归一化已经吃掉）
        assert!(mcmod_confident(
            &["xaero-world-map".into()],
            "[XWM] Xaero的世界地图 (Xaero's WorldMap)"
        ));
        assert!(!mcmod_confident(&["FTB Quests".into()], "FTBQuests Optimizer"));
        assert!(!mcmod_confident(
            &["just-enough-items".into()],
            "JustEnoughItems (Legacy)"
        ));
        assert!(!mcmod_confident(&["coppered-equipment".into()], "Exline's Copper Equipment"));
        assert!(!mcmod_confident(&["".into()], "钠 (Sodium)"));
    }

    #[test]
    fn name_forms_cover_prefix_and_paren_variants() {
        let f = name_forms("[JEI] JEI物品管理器 (Just Enough Items)");
        assert!(f.contains(&"jei物品管理器justenoughitems".to_string()), "{f:?}");
        assert!(f.contains(&"justenoughitems".to_string()), "{f:?}");
        // 衍生分支不给「括号前那一段」留口子
        assert!(!name_forms("JustEnoughItems (Legacy)").contains(&"justenoughitems".to_string()));
    }
}
