//! 关于页「鸣谢名单」的数据源：远端一份 JSON + 本机一份快照。
//!
//! 三条口径收在这一处，不留给前端各判一遍：
//!  - **端点只有一个常量**（`ACK_ENDPOINT`）。空串＝还没接上远端，这时刷新直接报错，
//!    界面上出「名单不可见 + 重新获取」；不静默返回空名单——"没有人贡献"和"没拿到数据"
//!    是两件事，混成一句就等于让用户怀疑工具坏了。
//!  - **快照落在应用自己的配置目录**（与 settings.json 同级），走 `task_engine::config_dir`
//!    那一个实现点，所以便携包里它跟着 exe 走。它**不是可清理缓存**：不进设置页的占用统计，
//!    也不被「清空全部」删掉——它是这份界面的出厂件，删了就等于让离线用户看到空页。
//!  - **读快照永不报错**：坏文件、只写了一半的文件、没有文件，是同一种状态（None）。
//!    进页先渲染快照、零网络，远端只在后台对账。
//!
//! 头像的 URL 来自 payload，所以它必须命中允许主机才算数。允许主机不从 payload 里来，
//! 由三类**构建期**来源组成：端点自己的主机、Minecraft 贴图域、`ACK_AVATAR_HOSTS` 点名的那批
//! （第三类是必需的：Cloudflare 的 `*.workers.dev` 配不了自定义域 ⇒ 端点与头像经常不同源，
//! 只按同源判会把所有自带头像一律抹成非法。细节见 `avatar_allowed`）。
//! 校验发生在**落盘之前**，缓存里那份因此一定是干净的。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::AppHandle;

use crate::core::downloader::{net_code, reqwest_code};
use crate::task_engine::config_dir;

/// 名单 JSON 的端点。空串＝未配置（此时 `ack_refresh` 必失败，界面走空态）。
///
/// 这里是**自定义域**而不是 `*.workers.dev`，两个原因都是实测出来的：
/// workers.dev 在简中网络被 DNS 投毒（解析回来的不是 Cloudflare 的地址，直连必挂），
/// 而它又配不了自定义域；顺带那个域还会被强制注入 `Cross-Origin-Resource-Policy`，
/// 把自带头像的 `<img>` 挡掉。换成自己的域以后，头像与端点**同源**，
/// 于是 `{ENDPOINT_HOST}` 直接可用、`ACK_AVATAR_HOSTS` 不需要设。
pub const ACK_ENDPOINT: &str = "https://poso.us.ci/contributors.json";

/// 本机快照的文件名，放在配置目录里
const ACK_FILE: &str = "contributors.json";

/// Minecraft 皮肤贴图的域：那条链是客户端直连它，不经过我们的端点
const MC_TEXTURE_HOST: &str = "textures.minecraft.net";

/// 未配置端点。单独一个码，是为了让它和真的网络故障在日志里分得开——界面上两者同一个处理。
const NOT_CONFIGURED: &str = "ack:not-configured";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AckPerson {
    /// 卡上显示的名字。对 Minecraft 玩家它同时也是取皮肤用的玩家名（显示名与 ID 同源，不分两个字段）
    pub name: String,
    /// 自带头像的 https 链接；取不到皮肤时回落它，两者都没有时前端落名字首字
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar: Option<String>,
    /// 是不是 Minecraft 账号：真＝去取皮肤做头像，假＝直接用 `avatar`
    #[serde(default)]
    pub minecraft_id: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AckList {
    /// 远端自己标的版本。与本机记的**不同即覆盖**，不做单调性判断——往回改也是改，
    /// 硬要求递增等于给自己留一条"改错了要发两个版本才能撤回"的路
    pub version: String,
    pub people: Vec<AckPerson>,
}

fn snapshot_path(app: &AppHandle) -> Option<PathBuf> {
    Some(config_dir(app)?.join(ACK_FILE))
}

/// 读本机快照。任何不顺利都算"没有快照"，不报错。
pub fn read_snapshot(app: &AppHandle) -> Option<AckList> {
    let raw = std::fs::read_to_string(snapshot_path(app)?).ok()?;
    serde_json::from_str::<AckList>(&raw).ok()
}

/// 版本没变就不写盘（省掉每次进页一次几 KB 的无意义重写）。写失败不影响本次结果：
/// 拿到的名单照样上屏，只是下次进页还得重新拉。
fn write_snapshot(app: &AppHandle, list: &AckList) {
    if let Some(prev) = read_snapshot(app) {
        if prev.version == list.version {
            return;
        }
    }
    let Some(path) = snapshot_path(app) else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(text) = serde_json::to_string(list) {
        let _ = std::fs::write(path, text);
    }
}

/// 从 `https://host[:port]/path` 里取小写主机名。手动解析不吃 `url` 依赖，
/// 并且比标准解析更狠：带 userinfo 的（`https://允许域@evil/`）直接判非法而不是当成合法主机。
fn host_of(raw: &str) -> Option<&str> {
    let rest = raw.strip_prefix("https://")?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    if authority.is_empty() || authority.contains('@') || authority.contains(' ') {
        return None;
    }
    let host = match authority.rsplit_once(':') {
        // 只有端口段全是数字才算 `host:port`，否则那个冒号不属于我们认的这一类主机（IPv6 字面量不碰）
        Some((head, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => head,
        Some(_) => return None,
        None => authority,
    };
    (!host.is_empty()).then_some(host)
}

/// 允许当头像源的三类主机：端点自己那个、MC 贴图域、以及构建时点名授权的那批。
///
/// 第三类是必需的，不是宽松化：**Cloudflare 的 `*.workers.dev` 配不了自定义域**（要自己的域
/// 就得加到账户里，而加域会要求把该账户下所有 Worker 切到新的 `*.<你的域>.workers.dev`）⇒
/// 端点主机与头像主机经常不同源，只按"同源"判会把所有自带头像一律抹成非法。
/// 通道：构建环境变量 `ACK_AVATAR_HOSTS`（逗号分隔的主机名）。它是**编译期**的，
/// 所以不匹配用户机器上有没有设这个变量——与 `__APP_VERSION__` 那类注入同一条性质。
fn avatar_allowed(raw: &str) -> bool {
    let Some(host) = host_of(raw) else { return false };
    if host == MC_TEXTURE_HOST {
        return true;
    }
    if endpoint_host().is_some_and(|allowed| host == allowed) {
        return true;
    }
    extra_avatar_hosts().any(|allowed| allowed == host)
}

/// 构建时额外授权的主机（见 `avatar_allowed`）。空串、空白项都当没有
fn extra_avatar_hosts() -> impl Iterator<Item = String> {
    option_env!("ACK_AVATAR_HOSTS")
        .unwrap_or("")
        .split(',')
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
}

/// 把 JSON 文本里的 `{ENDPOINT_HOST}` 换成端点主机。
///
/// Worker 那边已经换过一次（`env.ACK_CDN`），这里是**二次对账**：他部署时忘了设那个变量，
/// 或换 Worker 域名时只改了我们这一侧的常量，占位符就还是原样落过来。
/// 只在字符串值里出现，换完必定仍是合法 JSON；认不出主机（未配置端点）时原样返回。
fn expand_endpoint_host(raw: String) -> String {
    match endpoint_host() {
        Some(host) => raw.replace("{ENDPOINT_HOST}", host),
        None => raw,
    }
}

fn endpoint_host() -> Option<&'static str> {
    host_of(ACK_ENDPOINT)
}

/// 结构上收口：空名字的行丢掉，不合法的主机把 `avatar` 抹成 None（该行退回首字块，
/// 而不是整条消失——名单少一个人比头像少一张图严重得多）。
fn sanitize(mut list: AckList) -> AckList {
    list.people.retain(|p| !p.name.trim().is_empty());
    for p in list.people.iter_mut() {
        p.avatar = p
            .avatar
            .take()
            .filter(|a| avatar_allowed(a))
            .map(|a| a.trim().to_string());
    }
    list
}

/// 拉一次远端名单：请求 → 状态码归类 → 解析 → 校验 → 落盘。
/// 网络失败一律出 `net:` 码（前端 errors.ts 那套渲染口径），不原样抛 reqwest 的英文句子。
pub async fn fetch_list(client: &reqwest::Client) -> Result<AckList, String> {
    if ACK_ENDPOINT.is_empty() {
        return Err(NOT_CONFIGURED.to_string());
    }
    let resp = client
        .get(ACK_ENDPOINT)
        .send()
        .await
        .map_err(|e| reqwest_code(&e, ACK_ENDPOINT))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(net_code(ACK_ENDPOINT, status.as_u16()));
    }
    // 先按文本拿：占位符要在 parse 之前换掉（parse 再回写就等于用我们的 schema 改写他的 JSON）
    let raw = resp
        .text()
        .await
        .map_err(|e| reqwest_code(&e, ACK_ENDPOINT))?;
    let list: AckList = serde_json::from_str(&expand_endpoint_host(raw))
        // 形状不对不是网络问题，别蹭 `net:` 那套码：界面上同一处理，日志里要分得开
        .map_err(|_| "ack:bad-shape".to_string())?;
    Ok(sanitize(list))
}

pub fn store(app: &AppHandle, list: &AckList) {
    write_snapshot(app, list);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_endpoint_host_and_texture_host() {
        assert!(avatar_allowed(&format!("https://{MC_TEXTURE_HOST}/texture/abc")));
        assert!(!avatar_allowed("http://textures.minecraft.net/texture/abc")); // 明文一律拒
        assert!(!avatar_allowed("https://evil.example/a.png"));
        assert!(!avatar_allowed("https://evil.example/#@textures.minecraft.net"));
        // userinfo 伪装：主机段看着像允许域，实际指向别处
        assert!(!avatar_allowed(&format!("https://{MC_TEXTURE_HOST}@evil.example/x")));
        assert!(!avatar_allowed(""));
        assert!(!avatar_allowed("textures.minecraft.net/x"));
    }

    #[test]
    fn host_of_strips_port_and_rejects_stray_colon() {
        assert_eq!(host_of("https://a.example:8443/x"), Some("a.example"));
        assert_eq!(host_of("https://a.example/x"), Some("a.example"));
        assert_eq!(host_of("https://a.example:abc/x"), None);
        assert_eq!(host_of("https://:443/x"), None);
    }

    /// 这一条读的是**构建时**的三个来源。端点已填他自己的域（`poso.us.ci`）；
    /// `ACK_AVATAR_HOSTS` 保持不设——头像与端点同源，第二处配置本就不该存在。
    /// 换域名时这条会红，那是它在尽本分：提醒你端点常量和包要一起改
    #[test]
    fn endpoint_and_extra_hosts_are_build_time() {
        assert_eq!(endpoint_host().as_deref(), Some("poso.us.ci"));
        assert_eq!(extra_avatar_hosts().count(), 0, "同源 ⇒ 不需要额外授权头像主机");
        // 同源的头像算数；别人的域不算
        assert!(avatar_allowed("https://poso.us.ci/avatars/b.png"));
        assert!(!avatar_allowed("https://cdn.example/x.png"));
        assert_eq!(
            expand_endpoint_host("https://{ENDPOINT_HOST}/avatars/b.png".into()),
            "https://poso.us.ci/avatars/b.png"
        );
    }

    #[test]
    fn sanitize_drops_blank_names_and_bad_avatars() {
        let list = AckList {
            version: "1".into(),
            people: vec![
                AckPerson { name: "  ".into(), avatar: None, minecraft_id: false },
                AckPerson {
                    name: "Banxxx".into(),
                    avatar: Some("https://evil.example/x.png".into()),
                    minecraft_id: false,
                },
            ],
        };
        let out = sanitize(list);
        assert_eq!(out.people.len(), 1);
        assert_eq!(out.people[0].name, "Banxxx");
        assert_eq!(out.people[0].avatar, None);
    }

    #[test]
    fn wire_format_matches_frontend_field_names() {
        let json = r#"{"version":"7","people":[{"name":"Banxxx","minecraftId":true}]}"#;
        let list: AckList = serde_json::from_str(json).unwrap();
        assert!(list.people[0].minecraft_id);
        assert_eq!(list.people[0].avatar, None);
        // 回写时不合法的 avatar 已被抹掉 ⇒ 不出现 "avatar":null
        assert_eq!(serde_json::to_string(&list.people[0]).unwrap(), r#"{"name":"Banxxx","minecraftId":true}"#);
    }
}
