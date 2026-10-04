//! 应用更新（设置 · 外观与关于）。
//!
//! 为什么在 Rust 侧判：`1.0.0-beta.2` 与 `1.0.0-beta.10` 谁新，字符串比较一定比反，
//! 所以结论必须由 semver 算出来，前端只消费。
//!
//! 为什么不用官方 `tauri-plugin-updater`：它的下载址来自清单，运行时没法在「GitHub 原链不通」
//! 之后换镜像前缀重试，而更新包是十几 MB 级别——弱网这条只能在客户端解，这里复用与模组
//! 那条链同构的候选链（`core::downloader::source`）。且当前版本的插件没有公开的
//! `UpdateBuilder`，拿不到「只借它的装包引擎、检测仍用自己的」这条折中路。

pub mod fetch;
pub mod release;
pub mod verify;

use crate::models::UpdateChannel;

/// 更新包的 minisign **公钥**：`bundle.createUpdaterArtifacts` 签出的那批 `.sig` 就是配着它验的。
///
/// 空串是有意义的一档：**这一枚包没内置可信根** ⇒ 应用内更新一律判
/// `UpdateBlocked::MissingKey`，界面只给「打开发布页」。它是 §4 那条降级态的另一半——
/// release 里没 `.sig` 会挡住，这里有钥没配也会挡住，两条都不必让谁点一个必失败的按钮。
///
/// 私钥永远不进仓库、也不进这份文件：构建时由环境变量 `TAURI_SIGNING_PRIVATE_KEY`（钥匙内容本身，
/// 给路径或 URL 官方明说"不工作"）连同 `..._PASSWORD` 注入，出完包即撤。换公钥必须与构建脚本同一次改：
/// 只改一侧的话，旧包读不懂新钥签出的名，用户会停在「验签没过」那一档，
/// 而那档在界面上一句都修不了——所以换钥要连着发一版只为改钥的包。
const UPDATER_PUBKEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDZFMkVBMzEzMjUwMzExQTEKUldTaEVRTWxFNk11Ymg4Vnp4VnZ4blBWeVJpRXNwdExjTnNLNGFjcTAzVDAyTUo2RTlHRUQvVlAK";

/// 内置公钥（没有就回 `None`，调用方据此把「不能一键更新」的原因说清楚）。
/// 收成一个函数而不是让人直接读那枚 const：`None` 这一档要在两处（下载与判定）都拦得住。
pub fn updater_pubkey() -> Option<&'static str> {
    (!UPDATER_PUBKEY.is_empty()).then_some(UPDATER_PUBKEY)
}

/// 仓库 slug。**跨语言没法单源**：前端 `api.REPO_URL` 里是同一份的第二处字面量，
/// 迁仓库时两处一起改（那条注释也指向这里）。
const REPO_SLUG: &str = "banxxx/sideshift";

/// release 列表端点。**不是** `/releases/latest`：官方定义 latest 只返回
/// "most recent non-prerelease, non-draft release"，Beta 包在那条线上永远看不见自己该收的版本。
///
/// `per_page=30`：本渠道取最大版本要落在窗口内。30 条覆盖远超 30 天的发布密度，
/// 但**改发布节奏（比如一天一条）时要回来看这个数**，超窗口的老版本会被当成不存在。
pub fn releases_url() -> String {
    format!("https://api.github.com/repos/{REPO_SLUG}/releases?per_page=30")
}

/// 单条 release 端点（按 tag）——P1 下载取址用它，**不是**把 `releases_url()` 那份列表复用过来。
/// 两个理由：列表有 `per_page=30` 的窗口，用户点的可能是窗口外那条；而资产址是
/// `browser_download_url`，它带的是 GitHub 那一侧的时效，隔了几分钟再拿去下就可能在半路 403。
///
/// `tag` 由前端递回来，所以拼进路径前必须先过 `fetch::tag_ok`——这里不重复判，
/// 判据与「为什么这么判」只写在 fetch 那一处。
pub fn release_tag_url(tag: &str) -> String {
    format!("https://api.github.com/repos/{REPO_SLUG}/releases/tags/{tag}")
}

/// 允许当下载源的宿主。镜像档（P5）加进来时必须同时留在表里——
/// 验签打在落地字节上，所以镜像不可伪造，但**不接受 release 响应里回传的任何其它址**。
const ALLOWED_HOSTS: &[&str] = &[
    "github.com",
    "objects.githubusercontent.com",
    "raw.githubusercontent.com",
];

/// 这一轮实际订阅哪条线：用户选过用他选的，没选过看本机版本号带不带预发布位。
///
/// 检查与下载共用这一颗（`commands::check_update` / `prepare_update`），否则会出现
/// 「界面按 A 档推包、下载按 B 档验包」这种在两台机器上都自洽的错。
/// `Unspecified`（手改坏的那个占位值）先归位成档位默认，绝不能原样上到线上。
pub fn channel_for(picked: Option<UpdateChannel>, cur: &semver::Version) -> UpdateChannel {
    match picked.map(UpdateChannel::normalized) {
        Some(c) => c,
        None if cur.pre.is_empty() => UpdateChannel::Stable,
        None => UpdateChannel::Beta,
    }
}

/// URL 的宿主在不在白名单里。解不出 host（畸形 URL）也算不允许。
pub fn host_allowed(url: &str) -> bool {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_ascii_lowercase))
        .is_some_and(|h| ALLOWED_HOSTS.contains(&&h[..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 内置公钥的自检，而且是这轮改动**唯一**的错字指示器：这一串打错一个字符，
    /// 装了这个构建的所有用户都会停在「验签没过」那一档，而那档在界面上一句都修不了。
    /// 让它先在这里红，比让它在几万台机器上红便宜。
    #[test]
    fn builtin_pubkey_is_one_line_and_decodes() {
        let key = updater_pubkey().expect("这一版起就该内置可信根了");
        // 换行/首尾空白是手贴最常见的坏法：解不开还算运气好，解开了另一枚才是真事故
        assert!(!key.contains('\n') && !key.contains('\r'), "内联串必须是一行");
        assert_eq!(key, key.trim(), "公钥串不该带首尾空白");
        super::verify::decode_public_key(key).expect("内置公钥该解得开");
    }
}
