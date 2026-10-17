//! 本机开服验证的模型：会话快照、六步清单、验证日志行、副本模组条目。
//! 与 `ConversionReport.checks`（打包后离线对账，零子进程）分家：这一档起真进程、并且改的是解包副本。

use serde::{Deserialize, Serialize};

use crate::l10n::Msg;
use crate::models::LogLevel;

/// 会话状态。稿里的「通过带警告」不占一档：它仍是 `passed`，只是 `warnings` 非空
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum VerifyStatus {
    /// 还没跑过（副本也不在）
    Ready,
    /// 本机条件不满足 ⇒ 结果不可判定，进程没起过
    Blocked,
    Running,
    Passed,
    Failed,
    /// 到达内置超时，进程被终止：不判包坏
    Timeout,
    Cancelled,
    /// 交付包不在了：六步第一步就断，后面全部未执行
    Missing,
}

/// 六步（每步都指得回代码，见 `core::verify_run` 模块头）
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum VerifyStepId {
    Env,
    Unpack,
    Reconcile,
    Loader,
    Launch,
    Wait,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum StepStatus {
    Pending,
    Running,
    Pass,
    Warn,
    Fail,
    /// 这一步在这一档根本不发生（Fabric 没有首启装加载器）
    Skipped,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct VerifyStep {
    pub id: VerifyStepId,
    /// 中文步骤名：前端按原文查表，与 `CheckResult::label` 同一口径
    pub label: String,
    pub status: StepStatus,
    /// 已走到的那几步的一句话摘要（带真实数字）
    pub note: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note_msg: Option<Msg>,
}

/// 日志行的来源标签：界面按它着色，与稿里的 `env/unpack/verify/loader/java/server/mixin` 同源
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum VerifySource {
    Env,
    Unpack,
    Verify,
    Loader,
    Java,
    Server,
    Mixin,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct VerifyLogLine {
    pub time: String,
    pub source: VerifySource,
    /// 后端渲染好的中文整句（复制日志与缺目录条目时的兜底）
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_msg: Option<Msg>,
    pub level: LogLevel,
}

/// 副本 `mods/` 里的一枚模组。`file` 是磁盘上的名字（停用那枚带 `.disabled`），
/// `name` 是去掉那层后缀的名字 ⇒ 界面两档各列各的，切档不用重新问后端
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VerifyModEntry {
    pub file: String,
    pub name: String,
    pub enabled: bool,
    pub size_bytes: u64,
}

/// 失败现场：只摊证据，不给处置建议（错因不止模组一种）
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct VerifyFailure {
    pub step: VerifyStepId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// 报错行里点到的 jar 名（去重、截断）；一行都没有 ⇒ 空
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub jars: Vec<String>,
    /// 最近的报错原文，时间顺序；行号是本次输出流的序号
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lines: Vec<VerifyOutputRef>,
}

/// 一句指向输出流的话：`line` 从 1 起，是我们自己那条流的序号，不是 `server.log` 的行号
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct VerifyOutputRef {
    pub line: u32,
    pub text: String,
}

/// 服务端自己喊出来的 WARN（原文，不归我们翻）
pub type VerifyWarning = VerifyOutputRef;

/// 一次验证的完整快照：页面一次拿全，进度事件只带增量行
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct VerifySession {
    pub task_id: String,
    pub status: VerifyStatus,
    pub steps: Vec<VerifyStep>,
    pub logs: Vec<VerifyLogLine>,
    /// 副本根目录绝对路径；空串 = 还没解出来（或已经回收）
    pub copy_dir: String,
    /// 副本占用字节（页面「本次验证」那张卡的残留数）
    pub copy_bytes: u64,
    pub mod_total: u32,
    pub mod_disabled: u32,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub elapsed_sec: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// 副本 `server.properties` 里的 `server-port`：读不到为 None（未跑起来那一档）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_port: Option<u16>,
    /// 本次输出流读到的总行数（日志环只留最近若干条，这个数说的是总量）
    pub log_lines: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<VerifyWarning>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<VerifyFailure>,
    /// 不可判定的原因，一条一句（带真实数字的那些走模板，界面翻得动）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blockers: Vec<Msg>,
    /// 失败定位卡的环境提示：日志里已经喊过，这里再摊一次（目前只有 eula 未同意那一句）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hints: Vec<Msg>,
    /// 「验证参数」卡那三格的来源：脚本里的 `-Xmx…` 原样、`online-mode`、本次就绪上限秒数。
    /// 前两项读包内原文（没解包也有），跑过一轮之后以副本那份为准；读不到为 None，界面写「未取到」
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jvm_xmx: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub online_mode: Option<bool>,
    pub timeout_sec: u64,
    /// 构建时是否已把加载器并进产物（`ConversionReport.installed`）：没有报告就是 None，
    /// 这一格决定「首启装加载器」写「不需要」还是「要在副本里跑安装器」
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loader_installed: Option<bool>,
}

impl VerifySession {
    /// 页面第一次进来、什么都还没做时的那一档。`timeout_sec` 由会话层从执行体那档上限传进来：
    /// 这一层不认识 `core`，但界面上那个「30 分钟 · 内置」必须跟真正生效的那个数同源
    pub fn ready(task_id: &str, timeout_sec: u64) -> Self {
        Self {
            task_id: task_id.to_string(),
            status: VerifyStatus::Ready,
            steps: Vec::new(),
            logs: Vec::new(),
            copy_dir: String::new(),
            copy_bytes: 0,
            mod_total: 0,
            mod_disabled: 0,
            started_at: None,
            finished_at: None,
            elapsed_sec: 0,
            exit_code: None,
            server_port: None,
            log_lines: 0,
            warnings: Vec::new(),
            failure: None,
            blockers: Vec::new(),
            hints: Vec::new(),
            jvm_xmx: None,
            online_mode: None,
            timeout_sec,
            loader_installed: None,
        }
    }
}

/// 模组启停的结果：会话里那几个数跟着变，页面不必再拉一次整包
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ModToggleResult {
    pub entry: VerifyModEntry,
    pub mods: Vec<VerifyModEntry>,
    pub mod_total: u32,
    pub mod_disabled: u32,
}
