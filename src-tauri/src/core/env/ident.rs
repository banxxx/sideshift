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

/// 文件名能给出的 slug 候选：原样 id，外加去掉尾部加载器词的裸 id
/// （`sodium-fabric` → 先试 `sodium-fabric` 再试 `sodium`；中文文件名整体不是 slug，丢弃）
pub fn slugs_from_file_name(file_name: &str) -> Vec<String> {
    let (id, _) = crate::core::detector::split_mod_file(file_name);
    let id = id.trim().to_lowercase();
    let mut out = Vec::new();
    if is_slug(&id) {
        out.push(id.clone());
        if let Some((head, tail)) = id.rsplit_once('-') {
            if matches!(tail, "fabric" | "forge" | "neoforge" | "quilt" | "legacy")
                && is_slug(head)
            {
                out.push(head.to_string());
            }
        }
    }
    out
}

/// 供 commands 层组装 Target 列表（保持取证口径单一）。
/// `env_trusted` = 整包 `files[].env` 有区分度（见 detector 的同名判定）。可信时，已声明的行
/// 不必再联网：裁决表里 client/server 任一有值就能定案，重复查只会白烧 300/5min 的限流额度。
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
}
