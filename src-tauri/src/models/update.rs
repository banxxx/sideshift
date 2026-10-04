//! 应用更新的跨进程载荷（Rust: `check_update`）。与 `src/lib/types/update.ts` 逐字段对齐。

use serde::{Deserialize, Serialize};

use super::UpdateChannel;

/// release 资产的种类。前端用它分图标与判「产物配齐没」，**不拿它做任何安装决策**。
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum UpdateAssetKind {
    /// `createUpdaterArtifacts` 出的签名安装包（`*-setup.exe`，本机真构建实测的产物名）——签名覆盖的是它
    Package,
    /// 与某个 Package 同名的 `<包名>.sig`
    Signature,
    /// 手工打的便携 zip（`SideShift-<版本>-portable-<arch>.zip`），一期没有应用内自动替换
    Portable,
    /// 其余（源码包、别的架构…）：列出来给用户在看下载页时认得出，但不参与判定
    Other,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAsset {
    pub name: String,
    /// 单位字节，来自 GitHub API 的 `asset.size`（实测值，不是估算）
    pub size: u64,
    pub kind: UpdateAssetKind,
    /// 宿主不在白名单里时为 false：仍然列出来，但界面上它不是可点的东西
    pub trusted: bool,
}

/// 不能走「应用内一键更新」的原因（种类码，给人看的那句话在前端 `errors.ts` 同族的表里）。
/// 这条降级态是刻意的：**缺件不许报错，只许退回「打开下载页」**——否则「签名密钥还没生成」
/// 这种过渡期会让用户点一个必失败的按钮。
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum UpdateBlocked {
    /// 本机是便携形态（exe 旁边就是 `portable.flag`），一期不给一键更新
    Portable,
    /// 这个构建里没内置公钥（`core::update::UPDATER_PUBKEY` 是空的）：
    /// 装不了的不是「这条 release 不好」，而是**我们没法证明它好**——它压在所有 release 侧
    /// 原因之上，因为换钥之前配得再齐的产物也不能落地
    MissingKey,
    /// 这条 release 里没有签名安装包产物
    NoPackage,
    /// 有安装包但没有同名的 `.sig`
    NoSignature,
    /// 包或签名的宿主不在白名单
    UntrustedHost,
}

/// 检查更新的结果。结论（有没有更新、能不能一键装）全部在 Rust 侧算完，
/// 前端只渲染——包括渠道：那一档的判据只许有一个实现点。
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    /// 本地版本（取自 tauri 的 package_info，与前端注入的 __APP_VERSION__ 同一个源）
    pub current: String,
    /// 该渠道下最新的那条 release；仓库还没发过 release、或本渠道一条都没有时为 null
    pub latest: Option<String>,
    /// 那条 release 的 **tag 原文**（`v` 前缀保留）。`prepare_update` 按它重新解析一次 release
    /// 再取址——把 tag 交出去，前端才不必猜「版本号前面有没有 v」，我们也不必猜回来
    pub tag: Option<String>,
    /// latest 严格新于 current 才算有更新：同版本、更老都不提示
    pub has_update: bool,
    /// 这一轮实际订阅的渠道（用户选过用他选的，没选过用版本号推的那档）——
    /// 界面必须说清「你在哪条线」，否则 Beta 用户看见 1.1.0 会以为自己该收到它
    pub channel: UpdateChannel,
    /// 这条 release 的网页址：「打开下载页」的出口，**永久保留**，不是一键更新的临时替身
    pub release_url: Option<String>,
    /// release 的发布时间（GitHub 原样串）。用来在界面上说明这条包是什么时候出的，
    /// 以及 P4 的发布冷却（刚推上去的包不急着推给用户）
    pub published_at: Option<String>,
    /// release 正文：远端文本，直显不翻译（见 i18n 的动态句口径：不进 source-lock）
    pub notes: Option<String>,
    pub assets: Vec<UpdateAsset>,
    /// 产物配齐、宿主可信、且这个构建内置了公钥 ⇒ 允许走应用内更新；假 ⇒ 界面只给「打开下载页」
    pub downloadable: bool,
    /// downloadable 为假且确有更新可装时的原因。`latest` 为空（还没发过 release）时为 null
    pub blocked: Option<UpdateBlocked>,
}

/// 取件（下载 → 验签 → 待定稿）这一段的生命周期。
///
/// **与 `UpdateInfo` 分两张契约是刻意的**：那张说的是「有没有新版本」，一次性、来自远端；
/// 这张说的是「这一版在本地办到哪一步了」，会变、可取消、能被清缓存打断。合成一张，界面就
/// 得在同一个对象里猜哪些字段此刻有意义。
/// `checking` / `available` / `blocked` 不在这里——那三档由 `check_update` 的返回值表达，
/// 界面读 `UpdateInfo`。
/// **也没有 `installing`**：装那一跳的最后一句是「退出进程」，窗口当场就没了，界面上不存在一段
/// 「安装中」；装成了没有由下次启动的 `UpdateOutcome` 说。
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum UpdateStage {
    /// 没有进行中的取件（初次进来、或取消之后）
    Idle,
    Downloading,
    /// 字节收齐了，正在流式验签——包体十几 MB，这一步读得完才敢说「已验签」
    Verifying,
    /// 已验签、已定稿，等着被装（P2 的 `install_update` 只接受这一档）
    Ready,
    Failed,
    /// 用户主动取消：半截文件已删，与 `Failed` 的区别在于界面不该给「重试」而该给「重新下载」
    Canceled,
}

/// 取件状态（Rust: `update_status` / 事件 `update://progress` 同一个载荷）。
///
/// 进度与阶段共用一个结构：一次 tick 只发一条事件，前端不必把两张表拼起来才知道「在下第几版」。
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub stage: UpdateStage,
    /// 这一轮办的是哪个版本（`None` = 没有轮次）。事件与查询都要能说出「这是谁的状态」，
    /// 否则切渠道、或者旧事件晚到一步就会把界面演错
    pub version: Option<String>,
    /// 已收字节（不含验签那一段的读盘：那条读的是本地文件，进度按下载口径算）
    pub downloaded: u64,
    /// 来自 GitHub 的 `asset.size`（实测值）。0 = 服务端没给长度，界面只能显示「已下载多少」
    pub total: u64,
    /// 失败原因：**只有种类码**（`net:` / `app:`），渲染在前端 `errors.ts` 一处
    pub error: Option<String>,
}

impl UpdateStatus {
    /// 「没有轮次」的那一档：字段一次给全，界面不用补默认值
    pub fn idle() -> Self {
        Self {
            stage: UpdateStage::Idle,
            version: None,
            downloaded: 0,
            total: 0,
            error: None,
        }
    }
}

/// 上一次「重启并安装」到底装成了没有。这一档只在**有过一次安装尝试**时才存在，
/// 所以它是 `Option<UpdateOutcome>`——没有账本时界面什么都不说，而不是报一句「没装上」。
///
/// 为什么靠「下次启动读盘对账」而不是安装器的回执：Windows 上 spawn 之后就没有我们了——
/// 官方 NSIS 在 `/S` 下会直接杀掉正在运行的 `SideShift.exe`（生成的安装脚本引的
/// `utils.nsh` 那段 `IfSilent → KillProcess`），任何当场回调都不会执行。
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum UpdateOutcomeKind {
    /// 本机版本已经等于那次试图装上去的版本
    Done,
    /// 没等到：安装器没跑成、跑到一半被人关掉、或产物被杀毒软件清掉了
    Unfinished,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct UpdateOutcome {
    pub kind: UpdateOutcomeKind,
    /// 那次试图装上去的版本
    pub attempted: String,
    /// 按下那颗钮时本机是哪一版
    pub previous: String,
}
