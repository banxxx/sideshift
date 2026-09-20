//! 跨进程数据模型：字段名/取值必须与 src/lib/types.ts 契约逐一对齐。
//! serde 约定：结构体 camelCase，枚举小写；可选字段 Option + skip_serializing_if。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LoaderKind {
    Fabric,
    Forge,
    NeoForge,
}

impl LoaderKind {
    pub fn as_label(self) -> &'static str {
        match self {
            LoaderKind::Fabric => "Fabric",
            LoaderKind::Forge => "Forge",
            LoaderKind::NeoForge => "NeoForge",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PackManifest {
    pub file_name: String,
    pub loader: LoaderKind,
    pub mc_version: String,
    pub mod_count: u32,
    pub size_bytes: u64,
    pub parsed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// 源包绝对路径：前端解析得到、创建任务时原样带回，后端据此定位包文件
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ModDisposition {
    Remove,
    Keep,
    Add,
}

/// 端信息的证据来源（前端据此显示「依据什么判定」，可信度从高到低）
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum EnvSource {
    /// mrpack files[].env——整合包作者显式声明，最权威
    Mrpack,
    /// jar 内 fabric.mod.json / quilt.mod.json 的 environment 字段
    JarMetadata,
    /// Modrinth 按文件 sha1 反查构建（POST /v2/version_files）
    ModrinthHash,
    /// Modrinth 项目级 client_side/server_side（未下载模组的回落）
    ModrinthProject,
    /// 模组名关键字表，仅兜底
    NameHeuristic,
    /// 无任何证据
    #[default]
    Unknown,
}

/// 某端的支持程度：required 必需 / optional 可选（能装但无收益）/ unsupported 不支持
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SideFlag {
    Required,
    Optional,
    Unsupported,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PlanMod {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loader: Option<String>,
    pub disposition: ModDisposition,
    pub client_only: bool,
    pub needs_review: bool,
    pub auto_supplement: bool,
    /// 源文件大小（字节）：mrpack fileSize / zip 条目大小；0 = 未知（外部新增构建期才解析）
    #[serde(default)]
    pub size_bytes: u64,
    /// 需真联网下载（有 URL 且物理不在源包内）；false = 包内/本地直取
    #[serde(default)]
    pub needs_download: bool,
    /// 本地添加的 .jar 绝对路径（「从本地添加」项专用，downloader 直接取本地文件）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_path: Option<String>,
    /// 在线添加时钉住的具体构建：构建时按所选 url/sha1/文件名取，不再解析最新版
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned: Option<PinnedVersion>,
    /// 依赖的其他方案行 id（mrpack depends 解析所得，供反向依赖警告）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends: Vec<String>,
    /// 行 ↔ 包内条目的精确锚（detector 写入，前端原样回传）；
    /// 同 id 多文件（如一个模组两个版本）时靠它锁定正确条目，缺省回落 id 顺序匹配
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub src_path: Option<String>,
    /// 本次处置的证据来源（Unknown = 未判定）；前端据此给出「依据 X 判定」的可信度标注
    #[serde(default)]
    pub env_source: EnvSource,
    /// 客户端支持度；None = 无证据
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_side: Option<SideFlag>,
    /// 服务端支持度；与 client_side 一起给出「客户端必需 / 服务端可选」这一直白证据。
    /// None = 无证据：行默认保留，且不标「待人工确认」（多留不炸服，误删才会）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_side: Option<SideFlag>,
}

/// 自动分类结果事件载荷（`plan://classified`）：离线层与在线层各推一次
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PlanClassified {
    /// 归属包名：前端按当前 manifest.fileName 校验，切包后的迟到事件丢弃
    pub file_name: String,
    /// 带全部证据层的完整方案（前端只套用到用户未手动改过的行）
    pub plan: Vec<PlanMod>,
    /// 仍无任何端证据的行数（前端据此提示「N 个模组未判定，已默认保留」）
    pub unresolved: u32,
    /// false = 在线层还没跑完（离线那次推送用），true 才是本轮最后一次事件
    pub done: bool,
    /// false = 在线层有请求失败，结论可能不完整（前端提示可重跑）
    pub complete: bool,
}

/// `classify_pack` 的同步返回：离线层结论 + 在线层还会不会再推一次事件。
/// 前端据此决定「自动分类中」是否继续转圈——命令返回只代表离线层跑完。
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PlanClassification {
    pub plan: Vec<PlanMod>,
    pub online_pending: bool,
}

/// 取件构成（阶段 3 计划确定后写入）：网络 / 包内 / 本地 / 缓存命中四类来源的诚实汇总。
/// 旧字段 downloaded/total 仍是「全部条目」计数，net* 才是真联网量
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct FetchTally {
    /// 全部取件条目数
    pub files: u32,
    /// 全部条目字节（大小未知条目计 0）
    pub bytes: u64,
    /// 需联网条目数
    pub net_files: u32,
    /// 需联网字节
    pub net_bytes: u64,
    /// 包内直取条目数
    pub pack_files: u32,
    /// 本地文件复制条目数
    pub local_files: u32,
    /// 计划阶段就命中下载缓存的条目数（零流量、零解压）
    pub cached_files: u32,
}

/// 正在进行中的动作类型：联网传输 / 本地打包
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ActivityKind {
    /// 联网收字节
    Net,
    /// 写 zip
    Zip,
}

/// 当前动作（只随进度事件走，不进日志环）：前端在日志区上方渲染成一条实时进度条。
/// 定案依据：日志量级 = 前端性能预算（十五轮），逐毫秒的进度绝不能写成日志行。
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ActivityInfo {
    pub kind: ActivityKind,
    /// 联网 = 当前文件名；打包 = 当前顶层目录名（包根散件记「包根」）
    pub subject: String,
    pub done_bytes: u64,
    /// 0 = 总量未知（响应无 Content-Length）
    pub total_bytes: u64,
    pub items_done: u32,
    /// 0 = 总量未知
    pub items_total: u32,
    /// 平均速率（字节/秒），按采样窗口算
    pub rate_bps: f64,
    /// 第几次尝试（1 起）；>1 说明前面失败过，前端要标出来
    pub attempt: u32,
}
/// 用户在添加那一刻选定的 Modrinth 构建（与版本行一一对应，保证方案显示版本 = 实际下载版本）
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PinnedVersion {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha1: Option<String>,
    pub file_name: String,
}

/// 下载量预估（core::estimate，与构建取件分类同源）：转换摘要卡「预计下载」数据源
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DownloadEstimate {
    /// 本次构建将新产生的网络字节（缓存命中已扣除，含加载器本体）
    pub download_bytes: u64,
    /// 包内 / 本地直取字节（不产生网络流量）
    pub from_pack_bytes: u64,
    /// false = 有源不可达（离线等），数字可能偏小，前端保留「估算」标注
    pub complete: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct ConversionOptions {
    pub mc_version: String,
    pub loader_version: String,
    pub java_version: String,
    pub memory_mb: u32,
    pub generate_scripts: bool,
    pub nogui: bool,
    pub agree_eula: bool,
    /* ---- 服务端设置（server.properties 高频字段） ---- */
    pub server_port: u16,
    pub motd: String,
    pub max_players: u32,
    /// survival | creative | adventure | spectator
    pub gamemode: String,
    /// peaceful | easy | normal | hard
    pub difficulty: String,
    pub online_mode: bool,
    pub level_seed: String,
    /* ---- 启动参数扩展 ---- */
    /// Aikar's flags：G1GC 调优参数组，拼入 start 脚本 JVM 参数
    pub use_aikar_flags: bool,
    /// 用户附加 JVM 参数（原样拼接）
    pub extra_jvm_args: String,
    /* ---- 单包输出覆写 ---- */
    /// 本次转换输出目录；空 = 用全局设置
    pub output_override: String,
    /* ---- 客户端保留目录 ---- */
    /// 需要原样带入服务端的包内目录：相对路径（任意层级，如 kubejs/client_scripts），按前缀匹配
    pub keep_dirs: Vec<String>,
}

impl Default for ConversionOptions {
    fn default() -> Self {
        Self {
            mc_version: String::new(),
            loader_version: String::new(),
            java_version: String::new(),
            memory_mb: 4096,
            generate_scripts: true,
            nogui: true,
            agree_eula: false,
            server_port: 25565,
            motd: "A SideShift powered Minecraft server".into(),
            max_players: 20,
            gamemode: "survival".into(),
            difficulty: "easy".into(),
            online_mode: true,
            level_seed: String::new(),
            use_aikar_flags: false,
            extra_jvm_args: String::new(),
            output_override: String::new(),
            keep_dirs: Vec::new(),
        }
    }
}

/// 包内可保留目录树节点（客户端保留目录弹窗数据源）；
/// keep_dirs 条目 = 从包根起算的相对路径（如 kubejs/client_scripts），按前缀匹配拷贝
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PackDirNode {
    /// 目录名（不含路径），如 client_scripts
    pub name: String,
    /// 该目录内文件数（递归，含子目录）
    pub file_count: u32,
    /// 子目录节点，名字升序
    pub children: Vec<PackDirNode>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PipelineStage {
    Parser,
    Detector,
    Downloader,
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
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct VersionOption {
    pub value: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ModSource {
    Modrinth,
    Curseforge,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ModSearchResult {
    pub id: String,
    pub name: String,
    pub description: String,
    pub author: String,
    pub downloads: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_url: Option<String>,
    pub source: ModSource,
    pub compatible: bool,
    pub already_added: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ModSearchPage {
    pub source: ModSource,
    pub total: u64,
    pub results: Vec<ModSearchResult>,
    pub page: u32,
    pub page_size: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ModSearchQuery {
    pub source: ModSource,
    pub text: String,
    /// 空串 = 全部版本（不加 versions facet）
    pub mc_version: String,
    /// None = 任意加载器（不加 categories facet；前端「任意加载器」传 null）
    #[serde(default)]
    pub loader: Option<LoaderKind>,
    #[serde(default)]
    pub category: Option<String>,
    pub page: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ModVersionEntry {
    pub id: String,
    pub version_number: String,
    pub mc_version: String,
    pub loader: LoaderKind,
    pub date: String,
    pub size_bytes: u64,
    pub recommended: bool,
    /// 该构建主文件的直链（在线添加时随版本一起钉住）
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha1: Option<String>,
    /// 服务端下载文件名（与方案行展示版本对应）
    pub file_name: String,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DownloadSource {
    Official,
    Bmclapi,
    Github,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub output_dir: String,
    pub cache_dir: String,
    pub strip_client_only: bool,
    pub verify_after_build: bool,
    pub download_source: DownloadSource,
    pub concurrency: u32,
    /// 方案自动分类时允许联网反查 Modrinth（sha1 批量 + 项目级端声明）。
    /// 旧 settings.json 无此字段 → default_fn 补 true，不能让整体反序列化失败丢用户设置
    #[serde(default = "default_online_classify")]
    pub auto_classify_online: bool,
}

fn default_online_classify() -> bool {
    true
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            output_dir: String::new(),
            cache_dir: String::new(),
            strip_client_only: true,
            verify_after_build: false,
            download_source: DownloadSource::Official,
            concurrency: 6,
            auto_classify_online: true,
        }
    }
}

impl AppSettings {
    pub fn defaults_for(home: &std::path::Path) -> Self {
        Self {
            output_dir: home.join("SideShift/output").display().to_string(),
            cache_dir: home.join("SideShift/cache").display().to_string(),
            strip_client_only: true,
            verify_after_build: false,
            download_source: DownloadSource::Official,
            concurrency: 6,
            auto_classify_online: true,
        }
    }
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
