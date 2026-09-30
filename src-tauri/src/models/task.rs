use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::l10n::Msg;
use crate::models::{ConversionOptions, FetchTally, PackManifest};

/// 正在进行中的动作类型：联网传输 / 本机装 loader / 本地打包
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ActivityKind {
    /// 联网收字节
    Net,
    /// 本机跑安装器：只有「已装出多少」，没有总量（安装器的输出不是接口）
    Install,
    /// 写 zip
    Zip,
}

/// 当前动作（只随进度事件走，不进日志环）：前端在日志区上方渲染成一条实时进度条。
/// 定案依据：日志量级 = 前端性能预算（十五轮），逐毫秒的进度绝不能写成日志行。
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ActivityInfo {
    pub kind: ActivityKind,
    /// 联网 = 当前文件名；本机安装 = 「Forge 1.20.1-47.4.10」这样的一行主体；打包 = 当前顶层目录名（包根散件记「包根」）
    pub subject: String,
    pub done_bytes: u64,
    /// 0 = 总量未知（响应无 Content-Length；本机安装恒为 0——安装器不报总量）
    pub total_bytes: u64,
    pub items_done: u32,
    /// 0 = 总量未知（本机安装同样恒为 0，实时条走不定态）
    pub items_total: u32,
    /// 平均速率（字节/秒），按采样窗口算
    pub rate_bps: f64,
    /// 第几次尝试（1 起）；>1 说明前面失败过，前端要标出来
    pub attempt: u32,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PipelineStage {
    Parser,
    Detector,
    Downloader,
    /// 本机跑 loader 官方安装器（只在开关打开且该 loader 有 installer 可跑时出现：
    /// Fabric 的加载器 jar 从版本表直取，没有这一步 ⇒ 整档跳过，不是「跳过报错」）
    Installer,
    Builder,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    Queued,
    Running,
    Success,
    Failed,
    Cancelled,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TaskLogLine {
    pub time: String,
    pub stage: PipelineStage,
    pub message: String,
    pub level: LogLevel,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PlanCounts {
    pub remove: u32,
    pub keep: u32,
    pub add: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ConversionTask {
    pub id: String,
    pub pack: PackManifest,
    pub options: ConversionOptions,
    pub status: TaskStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<PipelineStage>,
    pub progress: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub downloaded: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<u32>,
    /// 取件构成（阶段 3 计划落定后写入）：界面据此区分「下载中」与「取件中」
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fetch: Option<FetchTally>,
    /// 已完成的联网条目数（缓存命中不计）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub net_done: Option<u32>,
    /// 已取回字节（含包内/本地/缓存），供「已取 X MB」文案
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub done_bytes: Option<u64>,
    /// 当前动作（联网传输 / 打包进行中才有值）：渲染成日志区上方的实时条
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity: Option<ActivityInfo>,
    pub created_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<TaskError>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub counts: Option<PlanCounts>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_file_name: Option<String>,
    /// 本任务实际产物的绝对路径：同名包自动加序号后与「默认名」不同名，
    /// 重试时要认得自己那份（覆写自己的，不去抢别人的文件名）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_size_bytes: Option<u64>,
    pub logs: Vec<TaskLogLine>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TaskError {
    pub stage: PipelineStage,
    pub title: String,
    pub detail: String,
    /// 失败种类代码（`net:offline:api.modrinth.com` 这一串）：界面上给人看的是它翻出来的
    /// 那句本地化文案，`detail` 里那条带 URL/状态码的原句只进「复制诊断信息」。
    /// 归不了类的失败（缺 API Key 那两句本来就是中文整句）没有码 ⇒ 旧档缺这个字段也照样成立
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    pub retryable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempts: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_tail: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ConversionReport {
    pub task_id: String,
    pub output_file_name: String,
    pub output_size_bytes: u64,
    pub duration_sec: u64,
    pub removed: u32,
    pub kept: u32,
    pub added: u32,
    pub pending_review: Vec<String>,
    pub options: ConversionOptions,
    /// 打进 zip 的文件数（builder 实数，非估算）
    #[serde(default)]
    pub file_count: u32,
    /// 本次实际写入包根的文件（start.bat / eula.txt / server.properties / …）
    #[serde(default)]
    pub generated_files: Vec<String>,
    /// 与上面互斥的另一半：包内自带同名文件（保留内容里勾了它），本次**没有**按配置生成，
    /// 配置卡里对应的值没进产物。报告与「服务端设置」那几项要按它改口，否则是在播报没生效的数
    #[serde(default)]
    pub reused_root_files: Vec<String>,
    /// 启动脚本指向的 jar 名：Fabric 是官方服务端 jar（首启自装），未本机安装的 Forge/NeoForge 是 installer。
    /// 已装好的新式布局为 None —— 它靠 `libraries/` 下的参数文件启动，没有单一 jar 可指
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_jar: Option<String>,
    /// 本次把本机装好的 loader 树并进了产物：目标机不再需要联网首装（Java 仍然要有）
    #[serde(default)]
    pub installed: bool,
    /// 构建后静态自检结论（core::verify）；空 = 没开这个开关
    #[serde(default)]
    pub checks: Vec<CheckResult>,
    /// 探明「两条取链路都拿不到字节」而**没有**装进产物的那些模组名（缺件闸门放行后才非空）。
    /// 报告与 README 逐条列出来：少件的包是用户看过清单并同意过的产物，不是静默丢件
    #[serde(default)]
    pub skipped_mods: Vec<String>,
}

/// 自检单项结论的三态：通过 / 提示（不致命但值得看一眼）/ 未通过（产物确实缺东西）
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CheckStatus {
    Pass,
    Warn,
    Fail,
}

/// 构建后静态自检的一项结果。全离线：只核对「打进 zip 的东西齐不齐、坏没坏」，
/// 不起服务端进程——所以它不能承诺「能开服」，措辞也就不能写成校验通过=可运行
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CheckResult {
    /// 稳定标识（files / jars / deps / start / loader / root / keep）：前端按它排布，不认中文标题
    pub id: String,
    pub label: String,
    pub status: CheckStatus,
    /// 一句话结论（带真实数字），报告页直接显示
    pub detail: String,
    /// 同一句话的「模板 + 参数」，给界面查翻译目录用（见 `crate::l10n`）。
    /// `detail` 仍是后端渲染好的中文整句：日志、剪贴板、以及这一项缺失时的兜底。
    /// 磁盘上的旧任务快照没有这一项 → `None`，前端照旧显示 `detail`，行为与改造前逐字一致。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail_msg: Option<Msg>,
    /// 涉及的对象名（缺哪几个文件、哪几个 jar 坏了），已截断
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProgressEvent {
    pub task_id: String,
    pub stage: PipelineStage,
    pub progress: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub downloaded: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log: Option<TaskLogLine>,
    /// 当前动作（联网传输 / 打包进行中）：前端直接据此刷新实时条，不必回拉任务列表
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity: Option<ActivityInfo>,
}
