//! GitHub release 列表 → 该渠道最新的那一条 → `UpdateInfo`。
//!
//! 渠道判据是**版本号里有没有预发布段**（`!ver.pre.is_empty()`），与本机版本号的判据同一条。
//! 这里刻意**不看 release 的 `prerelease` 复选框**：那是作者手填的，漏勾一次就会把
//! `1.0.0-beta.2` 当成「最新版」推给正式版用户（原 README 里唯一被标成"能真伤到用户"的那步），
//! 而 tag 里的版本号是自己会说话的那一份——发出去的包叫什么，它就属于哪条线。

use serde_json::Value;

use super::host_allowed;
use crate::core::downloader::net_code;
use crate::models::{UpdateAsset, UpdateAssetKind, UpdateBlocked, UpdateChannel, UpdateInfo};

/// 一条 release 里我们关心的部分。
#[derive(Debug)]
pub struct Release {
    pub version: semver::Version,
    /// 原始 tag（带 `v` 前缀的那一份）。P1 下载按 tag 重新解析一次 release，
    /// 拼 URL 用的就是它——**不是** `version.to_string()` 再拼回去，那等于假设 tag 只用我们这套写法
    pub tag: String,
    pub html_url: String,
    pub body: Option<String>,
    pub published_at: Option<String>,
    pub assets: Vec<Asset>,
}

impl Release {
    /// 签名安装包产物（`*-setup.nsis.zip`）。`createUpdaterArtifacts` 出的就是它，
    /// 签名覆盖的也是它——不是 exe、不是 portable zip
    fn package(&self) -> Option<&Asset> {
        self.assets.iter().find(|a| a.kind == UpdateAssetKind::Package)
    }

    /// 与那个包**同名加 `.sig`** 的签名。名字必须逐字对上：`.sig` 覆盖的是它签名那一刻读到的
    /// 那份产物，认错了文件名就等于没校验
    fn signature_for(&self, pkg: &Asset) -> Option<&Asset> {
        self.assets
            .iter()
            .find(|a| a.name == format!("{}.sig", pkg.name))
    }

    /// 下载要用的那一对（包, 签名）。缺件回 `None`——**说不出为什么缺**，那是 `gap()` 的活。
    /// 两条 find 与 `gap()` 共用，所以「界面说能装」与「下载真取到件」不会分叉。
    pub fn artifacts(&self) -> Option<(&Asset, &Asset)> {
        let pkg = self.package()?;
        let sig = self.signature_for(pkg)?;
        Some((pkg, sig))
    }

    /// 这条 release 为什么**不能**走应用内更新。`None` 表示「包 + 同名签名」都在且宿主都可信。
    ///
    /// 收成一张真值表（而不是在组装处铺一串 if）：判定只有一处，界面拿到的原因与实际拦下它的
    /// 那条分支必然是同一条。顺序也说明优先级——先说「你装的是便携版」，再说没接上密钥，
    /// 最后说缺什么件。**P1 下载前也读这张表**：那边拦下的与这里说出口的必须是同一条。
    pub fn gap(&self) -> Option<UpdateBlocked> {
        let Some(pkg) = self.package() else {
            return Some(UpdateBlocked::NoPackage);
        };
        if !pkg.trusted {
            return Some(UpdateBlocked::UntrustedHost);
        }
        let Some(sig) = self.signature_for(pkg) else {
            return Some(UpdateBlocked::NoSignature);
        };
        if !sig.trusted {
            return Some(UpdateBlocked::UntrustedHost);
        }
        None
    }
}

/// release 上的一个文件。
///
/// `url` 只在**下载那一跳**有意义：P1 是重新按 tag 解析出来的那份 release（§8 防列表窗口过期），
/// 现取现用。跨 IPC 给前端的那份是 `UpdateAsset`，**没有**这个字段——界面拿着一个可能已过期、
/// 而且我们并不信任的地址去自己下载，等于把白名单与验签这两道闸绕到客户端外面去。
#[derive(Debug)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
    pub kind: UpdateAssetKind,
    pub trusted: bool,
}

/// 只按文件名分种类：GitHub 不给产物类型，而产物名由构建链定死（见 `pnpm portable` 与
/// `createUpdaterArtifacts`）。认不出的一律 `Other`，宁可不给一键更新也不猜。
fn classify(name: &str) -> UpdateAssetKind {
    if name.ends_with("-setup.nsis.zip.sig") {
        UpdateAssetKind::Signature
    } else if name.ends_with("-setup.nsis.zip") {
        UpdateAssetKind::Package
    } else if name.ends_with("-portable-x64.zip") || name.ends_with("-portable-arm64.zip") {
        UpdateAssetKind::Portable
    } else {
        UpdateAssetKind::Other
    }
}

/// 一条 release JSON → `Release`。解不出 tag、或 tag 不是合法 semver 时回 `None`：
/// 宁可不推一条界面读不懂的，也不要它卡在比较里当"最新版"（比如手抖打了 `v1.0`）。
/// 草稿由调用方判——列表那边要跳过，单条那边走的是「这条不存在」的错，语义不一样。
fn release_from(r: &Value) -> Option<Release> {
    let tag = r["tag_name"].as_str()?;
    let version = semver::Version::parse(tag.trim_start_matches('v')).ok()?;
    let assets = r["assets"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| {
                    let name = x["name"].as_str()?;
                    let url = x["browser_download_url"].as_str()?;
                    Some(Asset {
                        name: name.to_string(),
                        url: url.to_string(),
                        size: x["size"].as_u64().unwrap_or(0),
                        trusted: host_allowed(url),
                        kind: classify(name),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Some(Release {
        version,
        tag: tag.to_string(),
        html_url: r["html_url"].as_str().unwrap_or("").to_string(),
        body: r["body"].as_str().map(str::to_string),
        published_at: r["published_at"].as_str().map(str::to_string),
        assets,
    })
}

/// 响应体 → release 列表。非数组（空仓库时 GitHub 回的是 `{"message":"Not Found"}` 对象）
/// 要原样递到界面上——不能装作"已是最新版本"，但它那句英文不用：按同一口径归类。
/// 限流单独归 `busy`：两者给用户的下一动作不一样。
pub fn parse(body: &Value, url: &str) -> Result<Vec<Release>, String> {
    let list = body.as_array().ok_or_else(|| not_found_code(body, url))?;
    Ok(list
        .iter()
        // 草稿谁都不该收到
        .filter(|r| !r["draft"].as_bool().unwrap_or(false))
        .filter_map(release_from)
        .collect())
}

/// 响应体 → 那一条 release（`/releases/tags/{tag}`，下载取址时用）。
/// 同一个归类口径：仓库里没这个 tag、或它被标成草稿，都算「这条 release 不存在」，
/// 而不是「解析成功、内容是空的」——下载那一步拿不到 tag 就无从继续。
pub fn parse_one(body: &Value, url: &str) -> Result<Release, String> {
    if !body.is_object() || body["message"].is_string() || body["draft"].as_bool() == Some(true) {
        return Err(not_found_code(body, url));
    }
    release_from(body).ok_or_else(|| net_code(url, 404))
}

/// 「这条 release 我没能读成一个结论」的种类码：限流归 `busy`，其余归 404——
/// 两者给用户的下一动作不一样（等一会儿 vs 别等了）。
fn not_found_code(body: &Value, url: &str) -> String {
    let msg = body["message"].as_str().unwrap_or("");
    net_code(url, if msg.contains("rate limit") { 429 } else { 404 })
}

/// 这一条属不属于 Beta 那条线。
fn is_beta(ver: &semver::Version) -> bool {
    !ver.pre.is_empty()
}

/// 这条 release 是不是用户订阅的那档。**渠道判据只有这一个实现点**：
/// P1 下载前也要过它，否则「正式版用户点了一下」就能把 Beta 包装上机器。
pub fn belongs(r: &Release, channel: UpdateChannel) -> bool {
    is_beta(&r.version) == (channel == UpdateChannel::Beta)
}

/// 本渠道内版本号最大的那条（不看它是否比本机新——"已是最新 v1.0.1" 也要能说清比的是谁）。
pub fn latest_in_channel(releases: &[Release], channel: UpdateChannel) -> Option<&Release> {
    releases
        .iter()
        .filter(|r| belongs(r, channel))
        .max_by(|a, b| a.version.cmp(&b.version))
}

/// 组装结论。`portable` 由调用方给（`data_root::portable_root()` 的实测结果，不是猜的）；
/// `has_pubkey` 同理——那是**这个构建**里有没有内置可信根，与远端那条 release 无关。
pub fn build_info(
    current: String,
    cur: &semver::Version,
    channel: UpdateChannel,
    best: Option<&Release>,
    portable: bool,
    has_pubkey: bool,
) -> UpdateInfo {
    let assets: Vec<UpdateAsset> = best
        .map(|r| {
            r.assets
                .iter()
                .map(|a| UpdateAsset {
                    name: a.name.clone(),
                    size: a.size,
                    kind: a.kind,
                    trusted: a.trusted,
                })
                .collect()
        })
        .unwrap_or_default();

    // 本渠道一条 release 都没有 ⇒ blocked 也是 null：那是「还没发过版」，不是异常。
    // 拦下的顺序就是下一动作的优先级：便携形态与没接上密钥都是「这条链在我们这侧就断了」，
    // 不必再去看远端缺了什么件
    let blocked = match best {
        None => None,
        Some(_) if portable => Some(UpdateBlocked::Portable),
        Some(_) if !has_pubkey => Some(UpdateBlocked::MissingKey),
        Some(r) => r.gap(),
    };
    let downloadable = best.is_some() && blocked.is_none();

    UpdateInfo {
        current,
        latest: best.map(|r| r.version.to_string()),
        // 下载那一跳按 tag 重新解析 release：把 tag 交出去，前端才不必去猜「版本号前面有没有 v」
        tag: best.map(|r| r.tag.clone()),
        has_update: best.is_some_and(|r| &r.version > cur),
        channel,
        release_url: best.map(|r| r.html_url.clone()).filter(|s| !s.is_empty()),
        published_at: best.and_then(|r| r.published_at.clone()),
        notes: best.and_then(|r| r.body.clone()).filter(|s| !s.trim().is_empty()),
        assets,
        downloadable,
        blocked,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(tag: &str, draft: bool, prerelease: bool, assets: &[(&str, bool)]) -> Value {
        let list: Vec<Value> = vec![serde_json::json!({
            "tag_name": tag,
            "draft": draft,
            "prerelease": prerelease,
            "html_url": "https://github.com/banxxx/sideshift/releases/tag/x",
            "published_at": "2026-10-01T00:00:00Z",
            "body": "说明",
            "assets": assets.iter().map(|(n, ok)| serde_json::json!({
                "name": n,
                "size": 1000,
                "browser_download_url": if *ok {
                    format!("https://github.com/x/download/{n}")
                } else {
                    format!("https://evil.example/{n}")
                }
            })).collect::<Vec<_>>(),
        })];
        Value::Array(list)
    }

    fn v(s: &str) -> semver::Version {
        semver::Version::parse(s).unwrap()
    }

    /// 渠道判据来自 tag 里的版本号，**不是** release 的 prerelease 复选框：
    /// 漏勾预发布标志的那条不许被当成正式版推出去（原 README 唯一"能真伤到用户"的那步）
    #[test]
    fn channel_comes_from_the_tag_not_the_checkbox() {
        // 忘勾 prerelease 的 beta 包：正式版用户绝不能收到它
        let body = json("v1.1.0-beta.2", false, false, &[]);
        let releases = parse(&body, "u").unwrap();
        assert!(latest_in_channel(&releases, UpdateChannel::Stable).is_none());
        assert!(latest_in_channel(&releases, UpdateChannel::Beta).is_some());

        // 反向：勾了 prerelease 但 tag 是纯版本号 ⇒ 按 tag 归正式版那条线
        let body = json("v1.2.0", false, true, &[]);
        let releases = parse(&body, "u").unwrap();
        assert!(latest_in_channel(&releases, UpdateChannel::Beta).is_none());
        assert_eq!(
            latest_in_channel(&releases, UpdateChannel::Stable)
                .unwrap()
                .version,
            v("1.2.0")
        );
    }

    /// semver 不是字符串：`beta.10` 比 `beta.2` 新
    #[test]
    fn prerelease_ordering_is_semver() {
        let mut releases = Vec::new();
        for tag in ["v1.0.0-beta.2", "v1.0.0-beta.10", "v1.0.0-beta.3"] {
            let body = json(tag, false, true, &[]);
            releases.extend(parse(&body, "u").unwrap().into_iter().map(|mut r| {
                r.html_url = tag.into();
                r
            }));
        }
        let best = latest_in_channel(&releases, UpdateChannel::Beta).unwrap();
        assert_eq!(best.version, v("1.0.0-beta.10"));
        assert_eq!(best.html_url, "v1.0.0-beta.10");
    }

    /// 草稿与解不出 semver 的 tag 一律不进候选
    #[test]
    fn drafts_and_unparsable_tags_are_dropped() {
        let body = serde_json::json!([
            { "tag_name": "v1.5.0", "draft": true, "prerelease": false, "assets": [] },
            { "tag_name": "v1.0", "draft": false, "prerelease": false, "assets": [] },
            { "tag_name": "v1.4.0", "draft": false, "prerelease": false, "assets": [] }
        ]);
        let releases = parse(&body, "u").unwrap();
        assert_eq!(releases.len(), 1);
        assert_eq!(releases[0].version, v("1.4.0"));
    }

    /// 空仓库回的是对象不是数组 ⇒ 报错，不许顶一个假的"已是最新"
    #[test]
    fn not_found_object_is_an_error_not_a_clean_bill() {
        let body = serde_json::json!({ "message": "Not Found" });
        assert!(parse(&body, "https://api.github.com/x").unwrap_err().starts_with("net:"));
        let limited = serde_json::json!({ "message": "API rate limit exceeded" });
        assert!(parse(&limited, "u").unwrap_err().contains("busy"));
    }

    /// 包与同名 .sig 都在且宿主可信才允许应用内更新；缺件是降级不是报错
    #[test]
    fn downloadable_needs_the_package_and_its_signature() {
        let both = json(
            "v1.1.0",
            false,
            false,
            &[
                ("SideShift_1.1.0_x64-setup.nsis.zip", true),
                ("SideShift_1.1.0_x64-setup.nsis.zip.sig", true),
            ],
        );
        let r = parse(&both, "u").unwrap();
        let info = build_info(
            "1.0.0".into(),
            &v("1.0.0"),
            UpdateChannel::Stable,
            r.first(),
            false,
            true,
        );
        assert!(info.downloadable);
        assert_eq!(info.blocked, None);
        assert!(info.has_update);

        // 只有包没有 .sig（没生成签名密钥的那一期）
        let r = parse(
            &json(
                "v1.1.0",
                false,
                false,
                &[("SideShift_1.1.0_x64-setup.nsis.zip", true)],
            ),
            "u",
        )
        .unwrap();
        let info = build_info(
            "1.0.0".into(),
            &v("1.0.0"),
            UpdateChannel::Stable,
            r.first(),
            false,
            true,
        );
        assert!(!info.downloadable);
        assert_eq!(info.blocked, Some(UpdateBlocked::NoSignature));

        // 便携形态：有配齐的产物也不给一键更新（替换裸 exe 另立一期）
        let r = parse(&both, "u").unwrap();
        let info = build_info(
            "1.0.0".into(),
            &v("1.0.0"),
            UpdateChannel::Stable,
            r.first(),
            true,
            true,
        );
        assert_eq!(info.blocked, Some(UpdateBlocked::Portable));

        // 签名来自不在白名单的宿主 ⇒ 不可信
        let r = parse(
            &json(
                "v1.1.0",
                false,
                false,
                &[
                    ("SideShift_1.1.0_x64-setup.nsis.zip", true),
                    ("SideShift_1.1.0_x64-setup.nsis.zip.sig", false),
                ],
            ),
            "u",
        )
        .unwrap();
        let info = build_info(
            "1.0.0".into(),
            &v("1.0.0"),
            UpdateChannel::Stable,
            r.first(),
            false,
            true,
        );
        assert_eq!(info.blocked, Some(UpdateBlocked::UntrustedHost));
    }

    /// 没内置公钥时**远端配得再齐也不算能装**：这一档说的是我们这侧没接上可信根，
    /// 而且它压在所有 release 侧原因之上——换钥之前报「缺 .sig」会把人引向错误的那一侧
    #[test]
    fn no_builtin_key_outranks_every_release_gap() {
        let r = parse(&json("v1.1.0", false, false, &[]), "u").unwrap();
        // 一条什么产物都没有的 release：本应报 no-package，但缺钥先说
        let info = build_info(
            "1.0.0".into(),
            &v("1.0.0"),
            UpdateChannel::Stable,
            r.first(),
            false,
            false,
        );
        assert_eq!(info.blocked, Some(UpdateBlocked::MissingKey));
        assert!(!info.downloadable);

        // 便携形态更优先：那一档连下载完都装不了，报缺钥是给错的人看
        let info = build_info(
            "1.0.0".into(),
            &v("1.0.0"),
            UpdateChannel::Stable,
            r.first(),
            true,
            false,
        );
        assert_eq!(info.blocked, Some(UpdateBlocked::Portable));
    }

    /// 本渠道一条 release 都没有 ⇒ latest 为 null，且不算任何异常
    #[test]
    fn empty_channel_is_silence_not_a_fault() {
        let r = parse(&json("v1.1.0", false, false, &[]), "u").unwrap();
        let info = build_info(
            "1.0.0".into(),
            &v("1.0.0"),
            UpdateChannel::Beta,
            None,
            false,
            false,
        );
        assert_eq!(info.latest, None);
        assert!(!info.has_update);
        assert_eq!(info.blocked, None);
        assert!(r.iter().all(|x| x.html_url.starts_with("https://")));
    }

    /// P1 下载按 tag 重新解析单条：非数组的回包是错，不是「空的但正常」
    #[test]
    fn single_release_parse_refuses_a_list_and_a_message_object() {
        let one = json(
            "v1.1.0",
            false,
            false,
            &[
                ("SideShift_1.1.0_x64-setup.nsis.zip", true),
                ("SideShift_1.1.0_x64-setup.nsis.zip.sig", true),
            ],
        );
        // 端点回的是数组（比如把列表端点的响应误递过来）⇒ 不是单条
        let err = parse_one(&one, "https://api.github.com/x").unwrap_err();
        assert!(err.starts_with("net:"), "{err}");

        let body = serde_json::json!({ "message": "Not Found" });
        assert!(parse_one(&body, "https://api.github.com/x")
            .unwrap_err()
            .starts_with("net:notfound"));
        let limited = serde_json::json!({ "message": "API rate limit exceeded" });
        assert!(parse_one(&limited, "https://api.github.com/x")
            .unwrap_err()
            .contains("busy"));

        // 正常单条：tag 与产物址都从这一份里拿
        let single = &one[0];
        let rel = parse_one(single, "https://api.github.com/x").unwrap();
        assert_eq!(rel.tag, "v1.1.0");
        assert_eq!(rel.version, v("1.1.0"));
        let (pkg, sig) = rel.artifacts().unwrap();
        assert!(pkg.url.starts_with("https://github.com/"));
        assert_eq!(sig.name, format!("{}.sig", pkg.name));
    }
}
