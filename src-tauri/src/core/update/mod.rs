//! 应用更新（设置 · 外观与关于）。
//!
//! 为什么在 Rust 侧判：`1.0.0-beta.2` 与 `1.0.0-beta.10` 谁新，字符串比较一定比反，
//! 所以结论必须由 semver 算出来，前端只消费。
//!
//! 为什么不用官方 `tauri-plugin-updater`：它的下载址来自清单，运行时没法在「GitHub 原链不通」
//! 之后换镜像前缀重试，而更新包是十几 MB 级别——弱网这条只能在客户端解，这里复用与模组
//! 那条链同构的候选链（`core::downloader::source`）。且当前版本的插件没有公开的
//! `UpdateBuilder`，拿不到「只借它的装包引擎、检测仍用自己的」这条折中路。

pub mod check;
pub mod fetch;
pub mod install;
pub mod release;
pub mod verify;

use std::sync::OnceLock;
use std::time::Duration;

use crate::core::downloader::{net_code, reqwest_code, USER_AGENT};
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

/// 允许当下载源的宿主。镜像宿主也在表里（P5 落地，见 `ASSET_MIRRORS` 的说明）——
/// 验签打在落地字节上，所以镜像不可伪造，但**不接受 release 响应里回传的任何其它址**。
const ALLOWED_HOSTS: &[&str] = &[
    "github.com",
    "objects.githubusercontent.com",
    "raw.githubusercontent.com",
    // 下面两个是 ASSET_MIRRORS 的宿主：镜像档进候选链的前提是宿主先在这里登记
    "ghfast.top",
    "ghproxy.net",
];

/// release 资产的镜像前缀（P5）：**前缀 + 完整 URL** 的形态（`https://ghfast.top/https://github.com/...`）。
///
/// 2026-10 实测：两家都对 `/releases/download/` 透明代理（含 Range，206）；同期的 gh-proxy.com
/// 已连不通——**这张表会朽**，维护契约是「烂一家摘一家，官方直连永远殿后兜底」：
/// 镜像全死时官方候选还带着满额重试，最坏退回无镜像时代的行为，不会有更糟的档。
///
/// 信任模型（为什么敢让第三方镜像碰更新包）：验签打在**落地字节**的哈希上，镜像哪怕伪造整份
/// 响应也签不出我们的名——它至多「藏起新版本」或「递来一个旧签名包」，后者有 `update-downgrade`
/// 那道闸拦着。API JSON（版本号、文件清单）没有镜像：ghproxy 系实测全部拒绝代理
/// `api.github.com`，要补这一层只能自建反代（见 `api_json` 的注释）。
const ASSET_MIRRORS: &[&str] = &["https://ghfast.top/", "https://ghproxy.net/"];

/// 一个资产的下载候选（按顺序试）：镜像在前（国内秒下）、官方殿后（吃满重试预算的最耐用源）。
/// 官方址是 release 响应里回传的 `browser_download_url`，候选表自己不发明任何址
pub fn asset_candidates(url: &str) -> Vec<String> {
    let mut out: Vec<String> = ASSET_MIRRORS
        .iter()
        .map(|m| format!("{m}{url}"))
        .collect();
    out.push(url.to_string());
    out
}

/// 单发超时。`Downloader` 那颗的全局 120s 在这一档是黑洞——那是给大文件留的，
/// 「检查更新」挂两分钟等于把人逼去点别的。
const API_TIMEOUT: Duration = Duration::from_secs(10);

/// GitHub API JSON 的取法：**先直连，失败再跟随系统代理**，各一发、各掐 10 秒。
///
/// 为什么默认要绕开系统代理：GitHub 匿名 API 的限额是**按出口 IP** 算的 60 次/小时，
/// 它与"这条路通不通"是两件事。而 reqwest 会读 Windows 那个代理开关（`tauri-plugin-http`
/// 的 default 打开了 reqwest 的 `system-proxy` feature，feature 全图合并 ⇒ 整个 exe 跟着吃），
/// 公共节点那个 IP 是几百人共用的 ⇒ 2026-10-14 同机实测：走 `127.0.0.1:7890` 那发
/// `403` 且 `X-RateLimit-Remaining: 0`，同一时刻直连那发还剩 52；一小时后同一个节点 IP
/// 重置成 60、连着敲几发就只剩 14。⇒ **开着代理，"能不能查到更新"就变成了"点下去那一刻
/// 那个节点还剩几次"**。直连用的是你自己那份 60 次/小时，而默认节奏是 24 小时一发。
///
/// 只圈这一跳：两颗 client 都是本模块私有的。资产下载（`fetch::client()`）与全局
/// `Downloader` 那颗照旧吃系统代理，别的出口一个都不改。
/// 兜底那颗**每趟新建**、不做进程级缓存：「跟随系统代理」必须在点下去这一刻成立，
/// 缓存会把"中途关了代理"演成往一个已经没人在听的端口敲门。
/// 为什么这一层没有镜像：见 `ASSET_MIRRORS` 的说明（ghproxy 系当场拒绝代理 API JSON，
/// 2026-10-14 复测仍是 `403 "Invalid input."`）；要补只能自建反代。
pub(crate) async fn api_json(url: &str) -> Result<serde_json::Value, String> {
    let direct_err = match try_api(direct_client(), url).await {
        Ok(body) => return Ok(body),
        Err(code) => code,
    };
    tokio::time::sleep(Duration::from_millis(500)).await;
    let Ok(sys) = reqwest::Client::builder().user_agent(USER_AGENT).build() else {
        return Err(direct_err);
    };
    match try_api(&sys, url).await {
        Ok(body) => Ok(body),
        // 两发都不成时念**直连那一发**的码：那是主路，它的下一动作（查网络或代理）
        // 比兜底路那句"太频繁，等一会儿"更接近用户真该做的事——等下去不会变好
        Err(_) => Err(direct_err),
    }
}

/// 一发：非 2xx 就地归成种类码，绝不让 GitHub 的错误页混进 `release::parse` 靠猜 body。
async fn try_api(client: &reqwest::Client, url: &str) -> Result<serde_json::Value, String> {
    let resp = client
        .get(url)
        .timeout(API_TIMEOUT)
        .send()
        .await
        .map_err(|e| reqwest_code(&e, url))?;
    let status = resp.status();
    if !status.is_success() {
        let remaining = resp
            .headers()
            .get("X-RateLimit-Remaining")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        return Err(api_code(url, status.as_u16(), remaining));
    }
    // 2xx 但回的是 HTML 错误页（拦截页/镜像墓碑）：与连不上同档，交给调用方换下一颗
    resp.json::<serde_json::Value>()
        .await
        .map_err(|e| reqwest_code(&e, url))
}

/// 直连那颗。`.no_proxy()` 一次关掉两件事：既清空代理列表，也关掉"建 client 时去读系统代理"。
fn direct_client() -> &'static reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    C.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .no_proxy()
            .build()
            .expect("reqwest direct client")
    })
}

/// 非 2xx 的归类。限流单独认**响应头**：GitHub 用 `403` 回配额，而那份 body 是合法 JSON——
/// 靠猜 body 里那句英文会被换措辞或一份假 JSON 打穿。认成 `busy`（配额按小时重置，
/// 「过一会儿再试」在这条上是真话）而不是 `denied`（那句念成"权限被拒"，用户会去查密钥）。
fn api_code(url: &str, status: u16, remaining: &str) -> String {
    net_code(url, if status == 403 && remaining == "0" { 429 } else { status })
}

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

    /// 镜像候选的形状：前缀 + 完整 URL，官方殿后；镜像宿主必须在白名单里
    #[test]
    fn asset_candidates_put_mirrors_first_and_official_last() {
        let official = "https://github.com/banxxx/sideshift/releases/download/v1.0.0/x.exe";
        let cands = asset_candidates(official);
        assert_eq!(cands.len(), 3);
        assert!(cands[0].starts_with("https://ghfast.top/https://github.com/"), "{}", cands[0]);
        assert!(cands[1].starts_with("https://ghproxy.net/https://github.com/"), "{}", cands[1]);
        assert_eq!(cands[2], official, "官方永远殿后");
        // 白名单契约：镜像宿主不在表里 = 候选链成了摆设
        assert!(host_allowed(&cands[0]) && host_allowed(&cands[1]) && host_allowed(official));
    }

    /// 限流认的是**响应头**，不是 body 里那句英文：`403 + Remaining: 0` 要说成「太频繁」
    /// （配额按小时重置，等是对的动作），普通 403 才是「被拒」。2026-10-14 那次
    /// 「开着代理查不到更新」被念成「无法连接」，就是这两档没分开导致的误判。
    #[test]
    fn rate_limited_403_reads_as_busy() {
        let url = "https://api.github.com/repos/banxxx/sideshift/releases";
        assert_eq!(api_code(url, 403, "0"), "net:busy:api.github.com");
        assert_eq!(api_code(url, 403, "5"), "net:denied:api.github.com");
        assert_eq!(api_code(url, 429, ""), "net:busy:api.github.com");
        assert_eq!(api_code(url, 500, "0"), "net:server:api.github.com");
    }

    /// 真联网那一条（默认不跑）：`cargo test --lib update::tests::api_json -- --ignored --nocapture`
    ///
    /// 为什么值得留：这一版的判据是「**Windows 代理开关开着**的时候，检查更新还能不能查到版本」，
    /// 它只在真实注册表状态 + 真实出口 IP 下成立，离线单测钉不住，而界面上点一次要等构建。
    /// 顺带跑一条**对照组**（同一发用"跟随系统代理"那颗打＝改动前的行为）：它的结果由外部配额
    /// 决定，所以只打印不断言。它通了只代表那一刻那个节点还有余额，反证不了主路多余——
    /// 主路要的是"每次都能成"，不是"看运气"。
    #[test]
    #[ignore]
    fn api_json_reaches_github_with_the_system_proxy_on() {
        let url = releases_url();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");

        match rt.block_on(api_json(&url)) {
            Ok(v) => {
                let n = v.as_array().map(Vec::len).unwrap_or(0);
                println!("[主路] 通了：{n} 条 release（直连优先，代理只是退路）");
            }
            Err(code) => panic!("[主路] 没通：{code}"),
        }

        let sys = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .build()
            .expect("对照那颗 client");
        match rt.block_on(try_api(&sys, &url)) {
            Ok(_) => println!("[对照] 跟随系统代理这一发今天也通（那个节点还有配额）"),
            Err(code) => println!("[对照] 跟随系统代理这一发：{code}"),
        }
    }

    /// 两处公钥必须同串。构建器那份（`tauri.conf.json` 的 `plugins.updater.pubkey`，
    /// `createUpdaterArtifacts` 就是靠它验私钥配对）只影响出包，运行期这份才影响用户——
    /// 只改一处会产出一枚「签得上、客户端读不懂」的包，而那要等用户点了下载才暴露。
    #[test]
    fn conf_pubkey_is_the_same_string_as_the_builtin_one() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json");
        let raw = std::fs::read_to_string(path).expect("读不到 tauri.conf.json");
        let conf: serde_json::Value = serde_json::from_str(&raw).expect("tauri.conf.json 不是合法 JSON");
        let in_conf = conf["plugins"]["updater"]["pubkey"].as_str().unwrap_or("");
        assert_eq!(
            in_conf,
            updater_pubkey().expect("内置公钥不该为空"),
            "换钥要同一次改 tauri.conf.json 与 UPDATER_PUBKEY"
        );
    }
}
