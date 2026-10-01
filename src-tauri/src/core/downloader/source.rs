//! 下载源：镜像 URL 重写 + 回落候选。两个镜像各管一片资源域，互不替代：
//! - **BMCLAPI**（`bmclapi2.bangbang93.com`，`download_source` 档位控制）：只覆盖 Mojang 系
//!   （版本表、Fabric 两张版本表、Forge promotions/maven 与 installer、NeoForge installer jar）。
//! - **mcimirror**（`mod.mcimirror.top`）：模组**查询 API** 的透明反向代理——CurseForge **无条件**走它
//!   （免 Key 的世界里官方 API 除 `/categories` 外全 401，镜像是唯一能应答的源）；Modrinth 受
//!   `modrinth_mirror` 开关控制（镜像优先、官方兜底）。
//! - **文件 CDN 不在镜像之列**：实测 mcimirror 对文件请求一律 302 跳回官方 CDN
//!   （`cdn.modrinth.com` / `mediafilez.forgecdn.net`），重写只会多一跳；直连官方 CDN 免 Key 可达。
//! - Fabric `/server/jar`、`maven.neoforged.net/api/*` 均无镜像——每条请求都留「镜像 → 官方」回落。
//! - 伴生 `<jar>.sha1` 校验值一律走官方（见 `client::attach_side_sha1`）：校验锚点必须是权威源。

use crate::models::DownloadSource;

/// 官方前缀 → BMCLAPI 前缀（`download_source` 选了镜像才启用）。
/// 映射不到的 URL 原样请求，不做任何猜测
const BMCLAPI_PREFIXES: &[(&str, &str)] = &[
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

/// CurseForge 查询 API → mcimirror。**无条件启用**（见 `candidates` 的类型注释）：
/// 无 Key 的世界里官方 API 除 `/categories` 外全 401，这是唯一能应答的源
const CF_API_PREFIXES: &[(&str, &str)] = &[(
    "https://api.curseforge.com/",
    "https://mod.mcimirror.top/curseforge/",
)];

/// Modrinth 查询 API → mcimirror（`modrinth_mirror` 开关控制）
const MODRINTH_API_PREFIXES: &[(&str, &str)] = &[(
    "https://api.modrinth.com/",
    "https://mod.mcimirror.top/modrinth/",
)];

/// 请求候选（按顺序试）。规则：
/// - CF API：无条件 `[mcimirror]` 单候选——无 Key 的世界里官方不应答，多一条 403 候选只是白敲门；
/// - Modrinth API：`modrinth_mirror` 开时 `[mcimirror, 官方]`，关时 `[官方]`；
/// - Mojang 系：`download_source` 选了镜像时 `[BMCLAPI, 官方]`，否则 `[官方]`。
pub fn candidates(url: &str, source: DownloadSource, modrinth_mirror: bool) -> Vec<String> {
    if let Some((_, m)) = rewrite(url, CF_API_PREFIXES) {
        return vec![m];
    }
    let mut out = Vec::new();
    if modrinth_mirror {
        if let Some((_, m)) = rewrite(url, MODRINTH_API_PREFIXES) {
            out.push(m);
        }
    }
    if source.is_mirror() {
        if let Some((_, m)) = rewrite(url, BMCLAPI_PREFIXES) {
            out.push(m);
        }
    }
    out.push(url.to_string());
    out
}

/// 前缀匹配重写：返回 `(命中的官方前缀, 镜像 URL)`；没有对应端点时 None（不重写，直接打官方）
fn rewrite<'a>(url: &str, prefixes: &'a [(&'a str, &'a str)]) -> Option<(&'a str, String)> {
    // Fabric 的服务端 jar 是 meta 现拼的组合端点，镜像没实现（实测 404）
    if url.ends_with("/server/jar") {
        return None;
    }
    let (from, to) = prefixes.iter().find(|(f, _)| url.starts_with(f))?;
    Some((from, url.replacen(from, to, 1)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use DownloadSource::{Bmclapi, Official};

    #[test]
    fn official_source_offers_only_the_original_url() {
        let url = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
        assert_eq!(candidates(url, Official, false), vec![url.to_string()]);
    }

    #[test]
    fn modrinth_mirror_toggle_gates_the_mcimirror_candidate() {
        let url = "https://api.modrinth.com/v2/project/sodium/version";
        assert_eq!(
            candidates(url, Official, true),
            vec![
                "https://mod.mcimirror.top/modrinth/v2/project/sodium/version".to_string(),
                url.to_string()
            ]
        );
        assert_eq!(candidates(url, Official, false), vec![url.to_string()]);
    }

    #[test]
    fn curseforge_api_always_goes_through_the_mirror() {
        let url = "https://api.curseforge.com/v1/mods/search?gameId=432&pageSize=5";
        // 开关关掉也照走：无 Key 的世界里镜像是 CF 唯一能应答的源
        assert_eq!(
            candidates(url, Official, false),
            vec!["https://mod.mcimirror.top/curseforge/v1/mods/search?gameId=432&pageSize=5"
                .to_string()]
        );
    }

    #[test]
    fn file_cdns_are_never_rewritten() {
        // mcimirror 对文件请求 302 跳回官方 CDN（实测）：重写只会多一跳
        for url in [
            "https://cdn.modrinth.com/data/AANobbMI/versions/x/sodium.jar",
            "https://edge.forgecdn.net/files/8909/889/modelfix-1.21-1.10.jar",
        ] {
            assert_eq!(candidates(url, Official, true), vec![url.to_string()]);
        }
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
                candidates(official, Bmclapi, false),
                vec![mirrored.to_string(), official.to_string()]
            );
        }
    }

    #[test]
    fn endpoints_without_a_mirror_stay_official() {
        // Fabric 组合端点、NeoForge 的 /api 版本表原样
        for url in [
            "https://meta.fabricmc.net/v2/versions/loader/1.20.1/0.19.5/1.1.2/server/jar",
            "https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/neoforge",
        ] {
            assert_eq!(candidates(url, Bmclapi, true), vec![url.to_string()], "{url} 不该被改写");
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
