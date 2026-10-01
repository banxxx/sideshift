//! 内置模组名称词典（MC百科词条中文名 ↔ 平台 slug），构建期由 build.rs 把
//! `assets/mcmod-names.tsv` 压成 gzip blob 内嵌进 exe，首次使用解一次。
//!
//! 只干两件事：**按 slug 反查中文显示名**（列表与详情，零请求）、**把中文查询词改写成英文词**
//! （搜索那条链，两家源各一道闸，见 `crate::core::downloader::modrinth` 里的分发点）。
//! 不参与端判定的证据阶梯，也不写进方案/任务存档——存档里的模组名仍是平台原名。
//!
//! 归一化与百科联网那条腿共用 `norm_name`（剥英文所有格 + 只留字母数字 + 小写），
//! 两本账对同一句查询给同一个键。

use std::collections::HashMap;
use std::io::Read;
use std::sync::OnceLock;

use flate2::read::GzDecoder;

use crate::core::downloader::norm_name;

/// 构建期压出来的表（缺它编不过，不会出现「跑起来才发现词典是空的」）
static BLOB: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/mcmod-names.gz"));

static DICT: OnceLock<Dict> = OnceLock::new();

/// 三张表：slug 反查中文名、中文别名反查候选词条名、词条名 ↔ 它对齐的唯一 slug
struct Dict {
    by_slug: HashMap<String, String>,
    by_alias: HashMap<String, Vec<String>>,
    sole_slug: HashMap<String, Option<String>>,
}

fn dict() -> &'static Dict {
    DICT.get_or_init(load)
}

fn load() -> Dict {
    let mut buf = Vec::new();
    GzDecoder::new(BLOB)
        .read_to_end(&mut buf)
        .expect("内置词典解不开");
    let text = String::from_utf8(buf).expect("内置词典不是 UTF-8");
    let mut d = Dict {
        by_slug: HashMap::new(),
        by_alias: HashMap::new(),
        sole_slug: HashMap::new(),
    };
    // 表按词条 id 升序生成：同一个 slug 撞上两个词条时先到先得，结果与运行顺序无关
    for line in text.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut cols = line.split('\t');
        let name = cols.next().unwrap_or("").trim();
        let slug = cols.next().unwrap_or("").trim();
        if name.is_empty() {
            continue;
        }
        if !slug.is_empty() {
            let slug_lower = slug.to_lowercase();
            d.by_slug
                .entry(slug_lower.clone())
                .or_insert_with(|| name.to_string());
            match d.sole_slug.get(name) {
                None => {
                    d.sole_slug.insert(name.to_string(), Some(slug_lower.clone()));
                }
                // 一个词条名对齐多个 slug：改写时不能拿它当英文词，记 None（不猜）
                Some(Some(prev)) if *prev != slug_lower => {
                    d.sole_slug.insert(name.to_string(), None);
                }
                Some(_) => {}
            }
        }
        for key in alias_keys(name) {
            let names = d.by_alias.entry(key).or_default();
            if !names.iter().any(|n| n == name) {
                names.push(name.to_string());
            }
        }
    }
    d
}

/// 一个词条名登记成哪几个中文键：整串、去掉开头 `[缩写]` 组、括号前那段。
/// 与联网那条腿的 `name_forms` 刻意不是同一份口径——那边不取「括号前那一段」（怕把衍生分支的
/// 端声明安到本体头上），这边取，因为改写走的是**唯一性闸**：那个键下只要挂着给出**不同改写词**的
/// 多个词条名，当场就不改写（实测这种键有 140 个，如 `ae2附加` 同时是 AE2 Additions 与 AE2 Extras）。
///
/// **不按 `/` 再切一段**：百科里有 272 个词条名的前半本身带斜杠（`钠/Embeddium：附属 (…)`、
/// `Macaw 的家具 / 麦考的家具 (…)`）。切了确实能让「麦考的家具」这种第二别名查得到，
/// 但同一刀也把 `钠` 这个键挂上了三枚附属模组，把唯一命中变成歧义拒改——而 `钠` 恰恰是最该改写的词
fn alias_keys(name: &str) -> Vec<String> {
    let mut keys = vec![norm_name(name)];
    let bare = match name.find(']') {
        Some(p) if name.starts_with('[') => name[p + 1..].trim_start(),
        _ => name,
    };
    if bare != name {
        keys.push(norm_name(bare));
    }
    let head = split_bracket(bare).map(|(head, _)| head).unwrap_or(bare);
    keys.push(norm_name(head));
    keys.retain(|k| !k.is_empty());
    keys.sort();
    keys.dedup();
    keys
}

/// 拆出「括号前那段」与「括号里那段」（半角/全角括号都认）；没有成对括号返回 None
fn split_bracket(s: &str) -> Option<(&str, &str)> {
    for (open, close) in [('(', ')'), ('（', '）')] {
        if let (Some(a), Some(b)) = (s.rfind(open), s.rfind(close)) {
            if a < b {
                return Some((&s[..a], &s[a + open.len_utf8()..b]));
            }
        }
    }
    None
}

/// 这句查询里有没有中日韩字：改写只在「用户打了中文」时发生，英文词一律直查、不碰
fn has_cjk(text: &str) -> bool {
    text.chars().any(|c| {
        matches!(c,
            '\u{3040}'..='\u{30ff}'  // 日文假名
            | '\u{3400}'..='\u{4dbf}' // 扩展 A
            | '\u{4e00}'..='\u{9fff}' // 常用汉字
            | '\u{f900}'..='\u{faff}' // 兼容汉字
        )
    })
}

/// 按平台 slug 反查词条中文名；查不到返回 None，调用方留着平台原名
pub fn zh_name(slug: &str) -> Option<&'static str> {
    let slug = slug.trim().to_lowercase();
    if slug.is_empty() {
        return None;
    }
    dict().by_slug.get(&slug).map(|s| s.as_str())
}

/// 搜索结果那一栏（slug 可能缺省）→ 中文显示名，给构造 `ModSearchResult` 的两家各用一行
pub fn zh_name_of(slug: Option<&str>) -> Option<String> {
    slug.and_then(zh_name).map(str::to_string)
}

/// 把中文查询词改成查得动的英文词：**命中唯一才改**——同一中文键下所有候选必须给出同一个改写词。
/// 改写词优先取词条名括号里的英文原名，没带括号的只在它对齐到唯一 slug 时才用。
/// 只要有一个候选给不出词、或两个候选给出不同的词，就一个字不改、原样交回：
/// 改错的产物是一屏看着像结果的错东西，比空屏更难读
pub fn rewrite_term(query: &str) -> Option<String> {
    let q = query.trim();
    if q.is_empty() || !has_cjk(q) {
        return None;
    }
    let names = dict().by_alias.get(&norm_name(q))?.clone();
    let mut word: Option<String> = None;
    for name in names {
        let cand = english_part(&name).or_else(|| sole_slug(&name))?;
        match &word {
            None => word = Some(cand),
            Some(prev) if *prev == cand => {}
            Some(_) => return None,
        }
    }
    word
}

/// 词条名括号里的那段英文（没有成对括号或里面是空的就没有）
fn english_part(name: &str) -> Option<String> {
    split_bracket(name).map(|(_, en)| en.trim().to_string()).filter(|en| !en.is_empty())
}

/// 该词条名对齐到的**唯一** slug；多个或没有都返回 None（不拿猜出来的 slug 当查询词）
fn sole_slug(name: &str) -> Option<String> {
    dict().sole_slug.get(name).and_then(|v| v.as_ref().cloned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 内嵌的是真表，所以这些断言钉的是产物、不是夹具
    #[test]
    fn zh_name_resolves_both_slugs_of_a_split_entry() {
        // 「铁路」在百科里同时挂着本体与重编译版两个 slug，两侧都要查得到
        assert_eq!(zh_name("railcraft"), Some("铁路 (Railcraft)"));
        assert_eq!(zh_name("railcraft-reborn"), Some("铁路 (Railcraft)"));
        assert_eq!(zh_name("RAILCRAFT"), Some("铁路 (Railcraft)"), "slug 大小写不影响反查");
        assert_eq!(zh_name("no-such-mod-slug"), None);
        assert_eq!(zh_name(""), None);
    }

    #[test]
    fn rewrite_uses_the_english_name_in_parentheses() {
        assert_eq!(rewrite_term("钠").as_deref(), Some("Sodium"));
        assert_eq!(rewrite_term(" 钠 ").as_deref(), Some("Sodium"));
        assert_eq!(rewrite_term("工业时代2").as_deref(), Some("Industrial Craft 2"));
        // 同一中文键下挂着本体与子项目，但两枚给出的英文词**逐字相同** ⇒ 照样算唯一命中
        assert_eq!(rewrite_term("红石计划").as_deref(), Some("ProjectRed"));
    }

    #[test]
    fn rewrite_refuses_ambiguous_and_unknown_terms() {
        // `ae2附加` 同时是 AE2 Additions 与 AE2 Extras 两个模组：改写词不一致 ⇒ 一个字不改
        assert_eq!(rewrite_term("ae2附加"), None);
        assert_eq!(rewrite_term("这句中文词典里没有"), None);
        // 英文词不碰：改写只在用户打中文时发生
        assert_eq!(rewrite_term("sodium"), None);
        assert_eq!(rewrite_term(""), None);
    }

    #[test]
    fn cjk_gate_covers_kana_and_han_variants() {
        assert!(has_cjk("小地图"));
        assert!(has_cjk("インベントリ"));
        assert!(!has_cjk("minimap"));
        assert!(!has_cjk("Abc 123"));
    }

    /// 真机量测（不参与常规跑批）：词典「首次解 blob + 建表」与「已加载后单条查名」各花多久。
    /// **必须 `cargo test --release` 跑**——debug 下的 `HashMap<String,_>` 查表慢一个量级，读出来的数不算
    #[test]
    #[ignore = "量测耗时，只在 release 下有意义"]
    fn measure_load_and_lookup_cost() {
        use std::time::Instant;
        let t = Instant::now();
        let d = dict();
        let load = t.elapsed();
        // 已加载后的单条查名：一半真 slug（命中）+ 一半假 slug（未收录，走的是「查不到」那条路）
        let keys: Vec<String> = d
            .by_slug
            .keys()
            .step_by(2)
            .take(2000)
            .cloned()
            .chain((0..2000).map(|i| format!("no-such-mod-{}", i)))
            .collect();
        let t = Instant::now();
        let mut hits = 0usize;
        for k in &keys {
            hits += d.by_slug.contains_key(k) as usize;
        }
        let per = t.elapsed().as_nanos() as f64 / keys.len() as f64;
        // 驻留内存的下限：三张表里字符串的字节合计（不含 HashMap 桶、Vec 头、String 容量取整）
        let bytes = d.by_slug.iter().map(|(k, v)| k.len() + v.len()).sum::<usize>()
            + d.by_alias
                .iter()
                .map(|(k, ns)| k.len() + ns.iter().map(|n| n.len()).sum::<usize>())
                .sum::<usize>()
            + d.sole_slug
                .iter()
                .map(|(k, s)| k.len() + s.as_deref().map(str::len).unwrap_or(0))
                .sum::<usize>();
        println!(
            "[dict] 首次解表+建表 {load:?}（by_slug {} 条 / by_alias {} 键 / sole_slug {} 名）；已加载后单条查名 {per:.0} ns/次（{} 次里命中 {hits}）；字符串字节合计 {:.2} MB（驻留下限）",
            d.by_slug.len(),
            d.by_alias.len(),
            d.sole_slug.len(),
            keys.len(),
            bytes as f64 / 1_048_576.0,
        );
    }
}
