//! 模组身份 → Modrinth 项目引用推导（URL 里的 project_id、文件名/包内 id 猜出的 slug 候选）。

use crate::core::parser::PackFile;
use super::index::Target;

/// Modrinth CDN URL 形如 `https://cdn.modrinth.com/data/<project_id>/versions/<vid>/<file>.jar`
pub fn project_id_from_url(url: &str) -> Option<String> {
    let pid = url.split("/data/").nth(1)?.split('/').next()?;
    (!pid.is_empty() && pid.len() <= 16).then(|| pid.to_string())
}

/// 像 Modrinth slug/id 的样子：ASCII、长度够、只含 slug 合法字符
pub fn is_slug(s: &str) -> bool {
    s.len() >= 3
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// Modrinth 文件名里「不属于模组名」的尾部词：加进 slug 只会换来一次 404
const LOADER_WORDS: &[&str] = &["fabric", "forge", "neoforge", "quilt", "legacy"];

/// `1.20.1` / `mc1.20.1` / `v1.7` / `0.92.2+1.20.1` 这种「只有数字和分隔符」的段
fn is_version_token(token: &str) -> bool {
    let t = token
        .strip_prefix("mc")
        .or_else(|| token.strip_prefix('v'))
        .unwrap_or(token);
    t.chars().next().is_some_and(|c| c.is_ascii_digit())
        && t.chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '.' | '+' | '-' | '_'))
}

/// CurseForge 常见的驼峰整串 → Modrinth 的连字符 slug（`XaerosWorldMap` → `xaeros-world-map`）。
/// 只在小写/数字接大写的边界处切，全小写时与 `join("-")` 同形，调用方靠重复项自然丢掉
fn camel_joined(tokens: &[&str]) -> String {
    let mut out = String::new();
    for t in tokens {
        if !out.is_empty() {
            out.push('-');
        }
        for c in t.chars() {
            let boundary = c.is_uppercase()
                && out
                    .chars()
                    .next_back()
                    .is_some_and(|p| p.is_lowercase() || p.is_ascii_digit());
            if boundary {
                out.push('-');
            }
            out.extend(c.to_lowercase());
        }
    }
    out
}

fn push_candidate(out: &mut Vec<String>, slug: &str) {
    if is_slug(slug) && !out.iter().any(|s| s == slug) {
        out.push(slug.to_string());
    }
}

/// 文件名能给出的 slug 候选，从最「原样」的写法一路剥到最裸的：
/// - 按 `-`/`_`/空格 切段，版本号段整串丢掉（CurseForge 的 `XaerosWorldMap_1.39.9_Forge_1.20.1`
///   把版本夹在名字中间，留着它就只剩一次注定 404 的请求）
/// - 每剥掉一个尾部加载器词多一个候选（`sodium-fabric` 确有其项目，故先原样再裸 id）
/// - 只剩一段且是驼峰时补连字符写法（Modrinth 上 `xaeros-world-map` 才是它的 slug）
///
/// 中文文件名整体不是 slug，丢弃。候选按顺序试，越多越烧 `MAX_LOOKUP_REQUESTS` 预算，
/// 所以只加「有一种写法确实命中过」的那几刀。
pub fn slugs_from_file_name(file_name: &str) -> Vec<String> {
    let (id, _) = crate::core::detector::split_mod_file(file_name);
    let parts: Vec<&str> = id
        .trim()
        .split(['-', '_', ' '])
        .filter(|t| !t.is_empty() && !is_version_token(&t.to_lowercase()))
        .collect();
    let mut out = Vec::new();
    let mut cur = parts;
    loop {
        push_candidate(&mut out, &cur.join("-").to_lowercase());
        if cur.len() != 1 {
            // 尾部加载器词逐个剥掉再试；不再以加载器词结尾（或已经剥光）就收
            let ends_with_loader = cur
                .last()
                .is_some_and(|t| LOADER_WORDS.contains(&t.to_lowercase().as_str()));
            if !ends_with_loader {
                break;
            }
            cur.pop();
            continue;
        }
        // 单段的驼峰名：连字符写法是它在 Modrinth 上的真名，值得多一次请求
        push_candidate(&mut out, &camel_joined(&cur));
        break;
    }
    out
}

/// 供 commands 层组装 Target 列表（保持取证口径单一）。
/// `env_trusted` = 整包 `files[].env` 有区分度（见 detector 的同名判定）。可信时，已声明的行
/// 不必再联网：裁决表里 client/server 任一有值就能定案，重复查只是白烧请求数和整轮那 60 秒。
/// 全表刷成 required/required 的那种默认值包必须照查——那层声明本身就是错的。
pub fn targets_for(files: &[PackFile], env_trusted: bool) -> Vec<Target> {
    files
        .iter()
        .filter(|f| {
            !(env_trusted && (f.env_client.is_some() || f.env_server.is_some()))
        })
        .map(|f| Target {
            path: f.path.clone(),
            sha1: f.sha1.clone(),
            project_id: project_id_from_url(&f.url),
            slugs: slugs_from_file_name(&f.file_name),
            title: None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_candidates_drop_loader_suffix_and_non_ascii_names() {
        assert_eq!(
            slugs_from_file_name("sodium-fabric-0.5.13.jar"),
            vec!["sodium-fabric".to_string(), "sodium".to_string()]
        );
        assert_eq!(
            slugs_from_file_name("fabric-api-0.92.2+1.20.1.jar"),
            vec!["fabric-api".to_string()]
        );
        // 中文改名的 jar：文件名给不出任何 slug 线索（只能靠包内自报身份）
        assert!(slugs_from_file_name("万用皮肤补丁+1.5.4-mc1.20.1-fabric.jar").is_empty());
    }

    #[test]
    fn slug_candidates_split_underscores_and_ditch_version_tokens() {
        // `split_mod_file` 只认 `-`，下划线写法的版本号会夹在名字中间：整串当 slug 必然 404
        assert_eq!(
            slugs_from_file_name("Xaeros_Minimap_23.10.0_Fabric_1.20.1.jar"),
            vec!["xaeros-minimap-fabric".to_string(), "xaeros-minimap".to_string()]
        );
        // Modrinth 自己那套 `mc<版本>` 段也不是名字的一部分
        assert_eq!(
            slugs_from_file_name("appleskin-fabric-mc1.20.1-3.0.6.jar"),
            vec!["appleskin-fabric".to_string(), "appleskin".to_string()]
        );
    }

    #[test]
    fn slug_candidates_add_hyphenated_form_for_camel_cased_names() {
        // 驼峰整串在 Modrinth 上的 slug 是连字符写法：`xaerosworldmap` 查不到，`xaeros-world-map` 才查得到
        assert_eq!(
            slugs_from_file_name("XaerosWorldMap_1.39.9_Forge_1.20.1.jar"),
            vec![
                "xaerosworldmap-forge".to_string(),
                "xaerosworldmap".to_string(),
                "xaeros-world-map".to_string()
            ]
        );
        // 全小写名两种写法同形，不多烧一次请求
        assert_eq!(
            slugs_from_file_name("sodium_0_5_13.jar"),
            vec!["sodium".to_string()]
        );
    }
}
