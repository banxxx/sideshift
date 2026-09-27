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

use base64::Engine as _;
use futures::StreamExt as _;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
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

/// 要不要把这一份存成本机快照。**版本没变不写**（省掉每次进页一次几 KB 的无意义重写）；
/// **本机那份有人、新这份空着 ⇒ 也不写**。
///
/// 后者拦的是一种发布事故而不是恶意：坏形状在 `fetch_list` 就报错了，能走到这里的只有
/// 「合法 JSON + `people` 是空数组」这一种，而那通常是把只填了 `version` 的壳推上线
/// （或 KV 手抖清空）。快照是这份界面的**出厂件**，被空表覆盖之后，离线用户就永久只剩
/// 空态，本机没有任何出口能把它找回来。「他真的清空了名单」这一种意图不在这条路的覆盖范围里：
/// 那需要一个显式标记，而不是靠空数组猜——空数组压住的那一格永远可以由有人的那一版覆盖回来。
fn should_store(prev: Option<&AckList>, next: &AckList) -> bool {
    match prev {
        // 本机什么都没有 ⇒ 空名单也存，它比没有强（下次离线至少拿到同一份空表，行为一致）
        None => true,
        Some(prev) => prev.version != next.version && !(next.people.is_empty() && !prev.people.is_empty()),
    }
}

/// 落盘。写失败不影响本次结果：拿到的名单照样上屏，只是下次进页还得重新拉。
fn write_snapshot(app: &AppHandle, list: &AckList) {
    if !should_store(read_snapshot(app).as_ref(), list) {
        return;
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

/* ---------------- 正版皮肤：玩家名 → 贴图地址 ----------------
 *
 * **只回地址，字节不过这里**。前端拿地址直连 `textures.minecraft.net`（实测响应带
 * `Access-Control-Allow-Origin: *` ⇒ `<img crossOrigin>` + 画布回读都不脏），
 * 所以这台机器上没有任何一版安装包要背皮肤的出口流量。
 *
 * **为什么整条查询在 Rust、不在浏览器、也不走我们的 Worker**：
 *  - 浏览器打不了：`api.minecraftservices.com` 与 `sessionserver.mojang.com` 都不给 CORS 头
 *    （只有贴图 CDN 给），"名字 → UUID → 贴图地址"那两跳在 WebView 里必被同源策略挡死。
 *  - Worker 那条 `/skins` 实测三处不通，一次都拿不到地址：Cloudflare 的出口被 Mojang 判 `403`
 *    （机房 IP 段整片拦）、它只认 `https` 而 Mojang 的 payload 里给的是 `http`、
 *    它解 payload 找的是 `skins.model.url`/`skins.classic.url` 而 sessionserver 给的是
 *    `textures.SKIN.url`。⇒ 本机直查是目前唯一跑得通的路，也少一次可信第三方。
 *
 * 缓存落配置目录、与快照同一条性质（**不是**可清理缓存）：限流是按 IP 算的，
 * 没有它每次进关于页都要重打几十次 Mojang，而这里错的只是"皮肤旧了几天"。
 */

/// 皮肤地址缓存的文件名
const SKIN_FILE: &str = "skins.json";
/// 一次查询的名字上限：没有它，名单一长就是"进一次关于页把 Mojang 的每 IP 限额点着"
const SKIN_BATCH_MAX: usize = 64;
/// Mojang 的批量查档一次最多收 16 个名字（实测 17 个直接 `400 CONSTRAINT_VIOLATION`）
const BULK_CHUNK: usize = 16;
/// 并发度：几十人一轮在秒级出完，又不至于把限额一次烧光
const SKIN_CONCURRENCY: usize = 4;
/// 一条地址的保鲜期。玩家会改名会换皮肤，但这里错的方向只是"外观旧几天"，不是"错到永远"
const SKIN_TTL_S: i64 = 7 * 24 * 3600;

const BULK_URL: &str = "https://api.minecraftservices.com/minecraft/profile/lookup/bulk/byname";
const SESSION_URL: &str = "https://sessionserver.mojang.com/session/minecraft/profile/";

/// 合法玩家名（与 Worker 那条同一口径）。下限照样不设：拦错的代价是某个人的皮肤永远出不来，
/// 放过的代价只是一次 404——而 404 不落表
fn mc_name_ok(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 16
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// 表里的一行。`at` 存 unix 秒而不是 ISO 串：这里只比较大小，不参与任何显示
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkinRow {
    url: String,
    at: i64,
}

/// 键＝小写玩家名（Minecraft 名字大小写不敏感，显示用的原名由调用方自己拿着）
type SkinTable = HashMap<String, SkinRow>;

fn now_s() -> i64 {
    chrono::Utc::now().timestamp()
}

/// 一次批量查询的全部网络往返：分片查 UUID → 逐个查贴图地址。
/// 失败不报错——**回什么就是什么**：查不到的人不在返回的表里，对他们前端走"自带 avatar → 首字"那两层。
/// 429 与 5xx 都不落表（`fetch_skins` 只在拿到地址时才写行），所以"暂时查不到"不会被冻成"永远查不到"。
pub async fn resolve_textures(client: &reqwest::Client, names: &[String]) -> HashMap<String, String> {
    let mut uniq: Vec<String> = Vec::new();
    let mut seen: HashMap<String, ()> = HashMap::new();
    for n in names {
        let key = n.trim().to_ascii_lowercase();
        if !mc_name_ok(&key) || seen.insert(key.clone(), ()).is_some() {
            continue;
        }
        uniq.push(key);
        if uniq.len() >= SKIN_BATCH_MAX {
            break;
        }
    }

    let chunks: Vec<Vec<String>> = uniq.chunks(BULK_CHUNK).map(|c| c.to_vec()).collect();
    let pairs: Vec<(String, String)> = futures::stream::iter(
        chunks
            .into_iter()
            .map(|chunk| async move { bulk_lookup(client, &chunk).await }),
    )
    .buffer_unordered(SKIN_CONCURRENCY)
    .collect::<Vec<_>>()
    .await
    .into_iter()
    .flatten()
    .collect();

    futures::stream::iter(pairs.into_iter().map(|(name, uuid)| {
        let client = client.clone();
        async move {
            let url = texture_url(&client, &uuid).await?;
            Some((name, url))
        }
    }))
    .buffer_unordered(SKIN_CONCURRENCY)
    .filter_map(|x| async move { x })
    .collect::<HashMap<_, _>>()
    .await
}

/// 名字 → UUID。Mojang 对不存在的名是**静默省略**（不报错、不占位），所以这里一次请求换一批人
async fn bulk_lookup(client: &reqwest::Client, names: &[String]) -> Vec<(String, String)> {
    let resp = match client.post(BULK_URL).json(names).send().await {
        Ok(r) if r.status().is_success() => r,
        _ => return Vec::new(),
    };
    let body: Vec<ProfileRef> = match resp.json().await {
        Ok(b) => b,
        Err(_) => return Vec::new(),
    };
    body.into_iter()
        .map(|p| (p.name.to_ascii_lowercase(), p.id))
        .collect()
}

/// UUID → 贴图地址。三道门槛：payload 里必须有 `textures.SKIN.url`、主机必须正好是贴图 CDN、
/// 一律收成 https（Mojang 发的就是 `http://`，而 WebView 里 http 资源比 https 页面更难办）
async fn texture_url(client: &reqwest::Client, uuid: &str) -> Option<String> {
    let resp = client
        .get(format!("{SESSION_URL}{}", encode_uuid(uuid)))
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let profile: SessionProfile = resp.json().await.ok()?;
    let value = profile
        .properties
        .into_iter()
        .find(|p| p.name == "textures")?
        .value;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(value)
        .ok()?;
    let skin: serde_json::Value = serde_json::from_slice(&decoded).ok()?;
    skin_url(skin["textures"]["SKIN"]["url"].as_str()?)
}

/// Mojang 接受不带连字符的 UUID；万一名单里存的是带连字符的形式，这里剥掉再拼 URL
fn encode_uuid(uuid: &str) -> String {
    uuid.chars().filter(|c| *c != '-').collect()
}

/// 收口成 https 并只认 Mojang 那个静态 CDN。放过去就等于把一个远端可控的外链交给前端去加载
fn skin_url(raw: &str) -> Option<String> {
    let https = match raw.strip_prefix("http://") {
        Some(rest) => format!("https://{rest}"),
        None => raw.to_string(),
    };
    (host_of(&https)? == MC_TEXTURE_HOST).then_some(https)
}

/// 拉一次皮肤地址：读缓存 → 只查过期与缺失的那些 → 合并写回 → 回全量
/// （回全量而不是只回"这次新查到的"：前端不想知道地址是从哪一档来的，它只要一张对照表）
pub async fn fetch_skins(
    app: &AppHandle,
    client: &reqwest::Client,
    names: &[String],
) -> HashMap<String, String> {
    let wanted: Vec<String> = names
        .iter()
        .map(|n| n.trim().to_ascii_lowercase())
        .collect();
    let mut table = read_skins(app);
    let now = now_s();
    let stale: Vec<String> = names
        .iter()
        .zip(wanted.iter())
        .filter(|(_, key)| {
            table
                .get(key.as_str())
                .is_none_or(|row| now - row.at > SKIN_TTL_S)
        })
        .map(|(n, _)| n.clone())
        .collect();
    if !stale.is_empty() {
        let fresh = resolve_textures(client, &stale).await;
        // 只在新地址真拿到时才改写这一行：一次抖动不该把上次成功的结果抹掉，
        // 也不该把 `at` 推后（那会让"这次没查到"被当成"刚查过"，白等一整个 TTL）
        for (key, url) in fresh {
            table.insert(key, SkinRow { url, at: now });
        }
        write_skins(app, &table);
    }
    names
        .iter()
        .zip(wanted.iter())
        .filter_map(|(n, key)| {
            table
                .get(key.as_str())
                .map(|row| (n.clone(), row.url.clone()))
        })
        .collect()
}

fn read_skins(app: &AppHandle) -> SkinTable {
    let Some(dir) = config_dir(app) else {
        return SkinTable::new();
    };
    let Ok(raw) = std::fs::read_to_string(dir.join(SKIN_FILE)) else {
        return SkinTable::new();
    };
    // 坏文件＝没有文件：下次查询会整表重写，不需要在这里替它保留半份
    serde_json::from_str(&raw).unwrap_or_default()
}

fn write_skins(app: &AppHandle, table: &SkinTable) {
    let Some(dir) = config_dir(app) else { return };
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    if let Ok(text) = serde_json::to_string(table) {
        let _ = std::fs::write(dir.join(SKIN_FILE), text);
    }
}

/* ---------- 贴图字节的一份本机副本 ----------
 * 地址表只管"这个人名下现在是哪张贴图"（7 天保鲜），这份副本管"那张贴图的字节本身"：
 * 命中就一个字节都不用出去，换皮肤也不影响（新地址＝新文件名，旧那张躺在那儿等同名地址回来）。
 *
 * 网络那条路**没有**改到后端来：字节照旧由 WebView 直连 CDN 取（那台机器的代理只有浏览器吃，
 * 挪进 reqwest 会把"现在能显示的人"变成"显示不出来"）。后端只做存与取，所以这份缓存
 * 一次都不会让可达性变差，只会让它少一次。
 */

/// 贴图副本的目录（配置目录下的一级子目录，一人一个 `<内容哈希>.png`）
const SKIN_DIR: &str = "skins";
/// 一张贴图的字节上限。64×64 的 PNG 实测一两 KB，这条只是拦住"把一个远端可控的大响应写成文件"
const TEXTURE_MAX: usize = 256 * 1024;

/// 缓存文件的完整路径。三道门槛：本来就是 https 且主机正好是贴图 CDN（`skin_url` 那条口径）、
/// 路径段是 `…/texture/<哈希>`、哈希是 64 位小写十六进制。**文件名是从远端返回的 JSON 里来的**，
/// 不校验形状就是往配置目录里拼路径的口——Mojang 的地址确实长成这样，末段天生就是缓存键
fn texture_path(dir: &Path, url: &str) -> Option<PathBuf> {
    let (prefix, key) = url.split_once("/texture/")?;
    if !key.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()) {
        return None;
    }
    // `skin_url` 会把 http 升成 https，所以「原样吐回来」等价于「本来就是 https 且主机正好是贴图 CDN」
    if skin_url(prefix)? != prefix || key.len() != 64 {
        return None;
    }
    Some(dir.join(SKIN_DIR).join(format!("{key}.png")))
}

/// 读本机那份贴图字节，回 base64（前端把它拼成 `data:` 地址喂给 `<img>`）。
/// 没有这个文件、读不动、大小不对都算"没缓存"，调用方自己走去网络
pub fn read_texture(app: &AppHandle, url: &str) -> Option<String> {
    read_texture_in(&config_dir(app)?, url)
}

fn read_texture_in(dir: &Path, url: &str) -> Option<String> {
    let bytes = std::fs::read(texture_path(dir, url)?).ok()?;
    (!bytes.is_empty() && bytes.len() <= TEXTURE_MAX)
        .then(|| base64::engine::general_purpose::STANDARD.encode(&bytes))
}

/// 写一份贴图字节。`false`＝没写成（目录建不了、磁盘满、地址形状不对），但**这不是错误**：
/// 本次解码已经成功了，缓存没落下来只是下次还得打一趟 CDN
pub fn write_texture(app: &AppHandle, url: &str, b64: &str) -> bool {
    config_dir(app).is_some_and(|dir| write_texture_in(&dir, url, b64))
}

fn write_texture_in(dir: &Path, url: &str, b64: &str) -> bool {
    let Some(path) = texture_path(dir, url) else {
        return false;
    };
    let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64) else {
        return false;
    };
    if bytes.is_empty() || bytes.len() > TEXTURE_MAX {
        return false;
    }
    let Some(parent) = path.parent() else { return false };
    if std::fs::create_dir_all(parent).is_err() {
        return false;
    }
    std::fs::write(&path, bytes).is_ok()
}

#[derive(Deserialize)]
struct ProfileRef {
    id: String,
    name: String,
}

#[derive(Deserialize)]
struct SessionProfile {
    #[serde(default)]
    properties: Vec<SessionProp>,
}

#[derive(Deserialize)]
struct SessionProp {
    name: String,
    value: String,
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

    /// 贴图地址的三道门槛：主机必须正好是 Mojang 那个 CDN，协议一律收成 https。
    /// 放过去一个远端可控的外链，等于让前端去加载我们不认识的东西
    #[test]
    fn texture_urls_are_pinned_to_the_cdn() {
        assert_eq!(
            skin_url("http://textures.minecraft.net/texture/abc").as_deref(),
            Some("https://textures.minecraft.net/texture/abc")
        );
        assert_eq!(
            skin_url("https://textures.minecraft.net/texture/abc").as_deref(),
            Some("https://textures.minecraft.net/texture/abc")
        );
        assert_eq!(skin_url("https://evil.example/texture/abc"), None);
        assert_eq!(skin_url("https://textures.minecraft.net.evil.example/x"), None);
        // userinfo 伪装：主机段读起来像允许域，实际指向别处
        assert_eq!(skin_url("https://textures.minecraft.net@evil.example/x"), None);
        assert_eq!(skin_url(""), None);
    }

    #[test]
    fn mc_names_reject_the_obvious_garbage() {
        assert!(mc_name_ok("POSOO"));
        assert!(mc_name_ok("banxxx_9"));
        assert!(mc_name_ok("a")); // 下限故意不设（见 `mc_name_ok`）
        assert!(!mc_name_ok("Ban xxx")); // 空格
        assert!(!mc_name_ok("张三"));
        assert!(!mc_name_ok(""));
        assert!(!mc_name_ok("0123456789abcdef0")); // 17 个字符
    }

    /// 快照的覆盖闸门：本机那份**有人**时，一份合法的空名单不许把它抹掉
    /// （坏形状进不到这里，所以这一格只挡"发布事故"那一种）。
    /// 本机本来就空着 ⇒ 不拦，那时没有任何东西可失去，存一份空表反而让离线行为一致
    #[test]
    fn empty_release_never_wipes_a_populated_snapshot() {
        let list = |version: &str, people: &[&str]| AckList {
            version: version.into(),
            people: people
                .iter()
                .map(|name| AckPerson {
                    name: (*name).into(),
                    avatar: None,
                    minecraft_id: false,
                })
                .collect(),
        };
        let stored = list("7", &["Banxxx"]);
        let same = list("7", &["Banxxx"]);
        let grew = list("8", &["Banxxx", "POSOO"]);
        let emptied = list("9", &[]);

        // 没有快照：什么都存，包括第一份空名单
        assert!(should_store(None, &stored));
        assert!(should_store(None, &emptied));
        // 版本没变：不写（省掉每次进页一次无意义重写）
        assert!(!should_store(Some(&stored), &same));
        // 正常发版：写
        assert!(should_store(Some(&stored), &grew));
        // 这一条是本次加固的对象：新版本、空名单 ⇒ 出厂件保住
        assert!(!should_store(Some(&stored), &emptied));
        // 本机那份已经是空的 ⇒ 不再拦（否则一个"清空名单"的意图永远同步不进来）
        let empty_stored = list("7", &[]);
        assert!(should_store(Some(&empty_stored), &emptied));
        // 同理：清空之后再来一版有人的，照常写
        assert!(should_store(Some(&empty_stored), &grew));
    }

    /// 缓存表的线格式：小写键 + `{url, at}`。前端只吃 `Record<string,string>`，
    /// 这张表不出 Rust ⇒ 换形状不用动前端，但**换键名要记得**这是持久件（旧文件会被当坏文件丢掉）
    #[test]
    fn skin_table_round_trips() {
        let mut table = SkinTable::new();
        table.insert(
            "posoo".into(),
            SkinRow {
                url: "https://textures.minecraft.net/texture/abc".into(),
                at: 1_700_000_000,
            },
        );
        let text = serde_json::to_string(&table).unwrap();
        assert_eq!(
            text,
            r#"{"posoo":{"url":"https://textures.minecraft.net/texture/abc","at":1700000000}}"#
        );
        assert_eq!(serde_json::from_str::<SkinTable>(&text).unwrap(), table);
        // 坏文件＝没有文件，不报错
        assert!(serde_json::from_str::<SkinTable>("{").is_err());
    }

    /// 贴图副本的文件名是从**远端返回的地址**里取的，所以这道门槛是路径注入的闸口：
    /// 只认「https + 贴图 CDN + 末段 64 位小写十六进制」这一种形状，其余一律不落成文件
    #[test]
    fn texture_cache_path_pins_the_host_and_the_hash_shape() {
        let dir = Path::new("cfg");
        let hash = "9b49d068923369682cafc31f50f93cb35c437358cddebc7feff5566fb943e8b5";
        let want = dir.join(SKIN_DIR).join(format!("{hash}.png"));
        assert_eq!(
            texture_path(dir, &format!("https://{MC_TEXTURE_HOST}/texture/{hash}")).as_deref(),
            Some(want.as_path())
        );
        let no = |u: String| assert_eq!(texture_path(dir, &u), None, "不该认这份地址：{u}");
        no(format!("http://{MC_TEXTURE_HOST}/texture/{hash}")); // 没升 https 的原样
        no(format!("https://evil.example/texture/{hash}")); // 主机不对
        no(format!("https://{MC_TEXTURE_HOST}/texture/abc")); // 末段太短
        no(format!("https://{MC_TEXTURE_HOST}/texture/{}", hash.to_uppercase())); // 大写另算一份
        no(format!("https://{MC_TEXTURE_HOST}/skin/{hash}")); // 不是 texture/ 那一档也不认（末段虽对，路径段一并收紧）
        no(format!(
            "https://{MC_TEXTURE_HOST}/texture/{}settings.json",
            "..\\".repeat(17)
        )); // 正好 64 个字符的反斜杠穿越
    }

    /// 副本本身跑得通：写进去的字节 == 读回来的字节，且真的落在 `skins/<哈希>.png` 那一格。
    /// 这条是整份缓存唯一"能不能省一次网络"的判据，路径门槛上面那条已经管了
    #[test]
    fn texture_bytes_round_trip_on_disk() {
        let dir = std::env::temp_dir().join(format!("sideshift-ack-tex-{}", uuid::Uuid::new_v4()));
        let url = format!(
            "https://{MC_TEXTURE_HOST}/texture/9b49d068923369682cafc31f50f93cb35c437358cddebc7feff5566fb943e8b5"
        );
        let png = [0x89u8, b'P', b'N', b'G', 13, 10, 26, 10, 7, 3];
        let b64 = base64::engine::general_purpose::STANDARD.encode(png);

        assert!(write_texture_in(&dir, &url, &b64));
        assert_eq!(read_texture_in(&dir, &url).as_deref(), Some(b64.as_str()));
        // 落点就是内容哈希那一格，且没写到 `skins/` 之外去
        assert!(dir.join(SKIN_DIR).join("9b49d068923369682cafc31f50f93cb35c437358cddebc7feff5566fb943e8b5.png").is_file());
        assert!(!dir.join("settings.json").exists());

        // 空字节与坏 base64 都不落盘（返回 false，且不把上一次的结果弄坏）
        assert!(!write_texture_in(&dir, &url, ""));
        assert!(!write_texture_in(&dir, &url, "!!!not base64!!!"));
        assert!(!write_texture_in(&dir, &format!("https://evil/texture/{}", "a".repeat(64)), &b64));
        assert_eq!(read_texture_in(&dir, &url).as_deref(), Some(b64.as_str()));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
