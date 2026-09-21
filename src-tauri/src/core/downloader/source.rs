//! 下载源：官方与 BMCLAPI 国内镜像之间的 URL 重写 + 回落候选。
//!
//! **镜像的覆盖边界（2026-09-21 逐条 curl 实测，别照着这张表之外的想法加功能）**：
//! - 有镜像：piston-meta 的 MC 版本表、Fabric 的两张版本表（`fabric-meta/v2`）、
//!   Forge 的 `promotions_slim` / `maven-metadata` 与 installer jar、NeoForge installer jar。
//! - **没有镜像**：Fabric 的 `/server/jar` 组合端点（镜像 404）、`maven.neoforged.net/api/*`
//!   版本列表接口、以及**整个 Modrinth**（搜索、构建端声明、模组文件下载主体字节都在这里）。
//!
//! 所以每条请求都留一条「镜像 → 官方」的回落：既兜住上面两类没镜像的 URL，
//! 也兜住 BMCLAPI 对刚发布构建的同步滞后（新 jar 先 404，过一会儿才有）。
//! 伴生的 `<jar>.sha1` 校验值一律走官方（见 `client::attach_side_sha1`）：
//! 让镜像自证清白没有意义，校验锚点必须是权威源。

use crate::models::DownloadSource;

/// 官方前缀 → 镜像前缀（BMCLAPI `https://bmclapi2.bangbang93.com`）。
/// 映射不到的 URL 原样请求，不做任何猜测
const PREFIXES: &[(&str, &str)] = &[
    (
        "https://piston-meta.mojang.com/mc/",
        "https://bmclapi2.bangbang93.com/mc/",
    ),
    (
        "https://meta.fabricmc.net/v2",
        "https://bmclapi2.bangbang93.com/fabric-meta/v2",
    ),
    (
        "https://files.minecraftforge.net/",
        "https://bmclapi2.bangbang93.com/maven/",
    ),
    (
        "https://maven.minecraftforge.net/",
        "https://bmclapi2.bangbang93.com/maven/",
    ),
    // 只映射 releases 仓库；同域名下的 /api/ 版本列表接口没有镜像
    (
        "https://maven.neoforged.net/releases/",
        "https://bmclapi2.bangbang93.com/maven/",
    ),
];

/// 请求候选（按顺序试）：镜像在前、官方在后。选官方源、该 URL 无镜像对应时只有官方一条
pub fn candidates(url: &str, source: DownloadSource) -> Vec<String> {
    match mirror_of(url).filter(|_| source.is_mirror()) {
        Some(m) => vec![m, url.to_string()],
        None => vec![url.to_string()],
    }
}

/// 该 URL 的镜像地址；镜像没有对应端点时 None（不重写，直接打官方）
fn mirror_of(url: &str) -> Option<String> {
    // Fabric 的服务端一体化 jar 是 meta 现拼的组合端点，镜像没实现（实测 404）
    if url.ends_with("/server/jar") {
        return None;
    }
    let (from, to) = PREFIXES.iter().find(|(f, _)| url.starts_with(f))?;
    Some(url.replacen(from, to, 1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use DownloadSource::{Bmclapi, Official};

    #[test]
    fn official_source_offers_only_the_original_url() {
        let url = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
        assert_eq!(candidates(url, Official), vec![url.to_string()]);
    }

    #[test]
    fn mirrored_endpoints_keep_official_as_fallback() {
        let cases = [
            (
                "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json",
                "https://bmclapi2.bangbang93.com/mc/game/version_manifest_v2.json",
            ),
            (
                "https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json",
                "https://bmclapi2.bangbang93.com/maven/net/minecraftforge/forge/promotions_slim.json",
            ),
            (
                "https://maven.minecraftforge.net/net/minecraftforge/forge/1.20.1-47.4.10/forge-1.20.1-47.4.10-installer.jar",
                "https://bmclapi2.bangbang93.com/maven/net/minecraftforge/forge/1.20.1-47.4.10/forge-1.20.1-47.4.10-installer.jar",
            ),
            (
                "https://maven.neoforged.net/releases/net/neoforged/neoforge/21.1.72/neoforge-21.1.72-installer.jar",
                "https://bmclapi2.bangbang93.com/maven/net/neoforged/neoforge/21.1.72/neoforge-21.1.72-installer.jar",
            ),
        ];
        for (official, mirrored) in cases {
            assert_eq!(
                candidates(official, Bmclapi),
                vec![mirrored.to_string(), official.to_string()]
            );
        }
    }

    #[test]
    fn endpoints_without_a_mirror_stay_official() {
        // Fabric 组合端点、NeoForge 的 /api 版本表、以及 Modrinth 全部原样
        for url in [
            "https://meta.fabricmc.net/v2/versions/loader/1.20.1/0.19.5/1.1.2/server/jar",
            "https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/neoforge",
            "https://api.modrinth.com/v2/project/sodium/version",
        ] {
            assert_eq!(candidates(url, Bmclapi), vec![url.to_string()], "{url} 不该被改写");
        }
    }

    #[test]
    fn unknown_download_source_falls_back_to_official() {
        assert!(!DownloadSource::Unspecified.is_mirror());
        assert_eq!(DownloadSource::Unspecified.normalized(), Official);
        assert_eq!(DownloadSource::Bmclapi.normalized(), Bmclapi);
        // 老 settings.json 残留 "github" 只能落成 Unspecified，不能整份反序列化失败
        let s: DownloadSource = serde_json::from_str("\"github\"").unwrap();
        assert_eq!(s, DownloadSource::Unspecified);
    }
}
