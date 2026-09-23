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

/// 端信息的证据来源（前端据此显示「依据什么判定」）。
/// 可信度顺序见 `env::rank`：jar 自证 > 平台按构建 > 平台按项目 > 整合包声明 > 名称启发
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum EnvSource {
    /// mrpack files[].env——打包者填的第二手声明，整表无区分度时整层作废
    Mrpack,
    /// jar 内 fabric.mod.json / quilt.mod.json 的 environment 与 entrypoints 段
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

/// jar 字节码扫描给出的结构提示。它**不是**证据层级的一员：实测「引用了哪些 MC 类」
/// 区分不了两端（纯客户端模组照样大量引用 `net/minecraft/world/`），只有加载器 API 的
/// 注册形状可用，而它只够用来「少删一点」和「提示一句」，不够用来判定剔除。
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BytecodeHint {
    /// jar 里确有服务端注册（common setup / 注册表 / 网络 payload…）：名称关键字层被这道闸按住，未剔
    ServerCode,
    /// jar 只见客户端生命周期注册、不见任何服务端注册：形状像纯客户端，本轮不自动剔，只提示
    ClientOnlyShape,
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
    /// 更高可信层与整合包 `files[].env` 的裁决不一致：结论取高可信层，
    /// 冲突只作标注（打包者声明与模组作者声明本来就常互相打脸，得让人看得见）
    #[serde(default)]
    pub env_conflict: bool,
    /// 客户端支持度；None = 无证据
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_side: Option<SideFlag>,
    /// 服务端支持度；与 client_side 一起给出「客户端必需 / 服务端可选」这一直白证据。
    /// None = 无证据：行默认保留，且不标「待人工确认」（多留不炸服，误删才会）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_side: Option<SideFlag>,
    /// 字节码结构提示（只影响提示文案与名称层是否获准剔除，不改裁决口径）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytecode_hint: Option<BytecodeHint>,
}

/// 自动分类结果事件载荷（`plan://classified`）：离线层与在线层各推一次
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PlanClassified {
    /// 归属包名：前端按当前 manifest.fileName 校验，切包后的迟到事件丢弃
    pub file_name: String,
    /// 带全部证据层的完整方案（前端只套用到用户未手动改过的行）
    pub plan: Vec<PlanMod>,
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
    /// 打进 zip 的文件数（builder 实数，非估算）
    #[serde(default)]
    pub file_count: u32,
    /// 本次实际写入包根的文件（start.bat / eula.txt / server.properties / …）
    #[serde(default)]
    pub generated_files: Vec<String>,
    /// 启动脚本指向的 jar 名：Fabric 是服务端一体化 jar，Forge/NeoForge 是 installer
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_jar: Option<String>,
    /// 构建后静态自检结论（core::verify）；空 = 没开这个开关
    #[serde(default)]
    pub checks: Vec<CheckResult>,
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
    /// 稳定标识（files / jars / deps / start / root / keep）：前端按它排布，不认中文标题
    pub id: String,
    pub label: String,
    pub status: CheckStatus,
    /// 一句话结论（带真实数字），报告页直接显示
    pub detail: String,
    /// 涉及的对象名（缺哪几个文件、哪几个 jar 坏了），已截断
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<String>,
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
    /// 项目级两侧支持度（Modrinth `client_side`/`server_side`）：在线添加行的端标签
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_side: Option<SideFlag>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_side: Option<SideFlag>,
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
    /// 构建级 `environment` 换算出的两侧支持度：这个构建进服务端包要不要，添加前就能看到
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_side: Option<SideFlag>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_side: Option<SideFlag>,
}

/// 「从本地添加」单个 jar 的取证结果（在线/离线各层跑完后的两侧支持度）
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct AddedModSide {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_side: Option<SideFlag>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_side: Option<SideFlag>,
    pub env_source: EnvSource,
    /// jar 字节码结构提示（同 `PlanMod::bytecode_hint` 口径）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytecode_hint: Option<BytecodeHint>,
    /// 探测到的实际字节数（本地添加行补体积，摘要才不算空）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
    /// jar 内自报的模组 id（`fabric.mod.json:id` / `mods.toml:modId`）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mod_id: Option<String>,
    /// 自报显示名：文件名被改成中文时这才是可读名字
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DownloadSource {
    Official,
    Bmclapi,
    /// 旧 settings.json 里可能留着已下掉的档位（曾经的 github 源）：认成占位值，
    /// 不能让整份设置因为一个无效枚举值反序列化失败而全丢（见 persist::load_settings）
    #[serde(other)]
    Unspecified,
}

impl DownloadSource {
    /// 只有明确选了 BMCLAPI 才走镜像，占位值按官方
    pub fn is_mirror(self) -> bool {
        self == Self::Bmclapi
    }

    /// `other` 反序列化出来的占位值会被写成 "unspecified"，落盘前归一掉
    pub fn normalized(self) -> Self {
        if self == Self::Bmclapi {
            Self::Bmclapi
        } else {
            Self::Official
        }
    }
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

/// 缓存占用报表（设置页「存储与缓存」）。字段全部是**扫描实测**，没有估算值：
/// 每类都给「个数 + 字节」两栏，是因为只有字节看不出"清掉了几百个小文件"，
/// 只有个数又完全无法判断值不值得清。
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CacheUsage {
    /// 缓存目录绝对路径（外显用；与设置里的 cacheDir 同源）
    pub cache_dir: String,
    /// 目录不存在 = 还没下载过任何东西。此时所有计数为 0，前端不该报错
    pub exists: bool,
    /// 下载缓存（可复用）
    pub files_count: usize,
    pub files_bytes: u64,
    /// 其中最后一次使用早于 stale_days 前的
    pub stale_count: usize,
    pub stale_bytes: u64,
    /// 半截下载的临时文件（有任务在跑时为 0，见 busy）
    pub parts_count: usize,
    pub parts_bytes: u64,
    /// 注册表里已无此任务 id 的暂存目录（个数按目录算，不是一个文件算一个）
    pub orphan_count: usize,
    pub orphan_bytes: u64,
    /// 空壳目录：零字节，但要让用户看见"清理确实收尾了"
    pub empty_dirs: usize,
    /// 有任务正在排队或运行：前端据此禁掉缓存清理，并把 parts 那一栏改口径说明
    pub busy: bool,
    pub stale_days: u64,
}

/// 一次清理的实际结果。字节数来自删除前逐文件读到的 metadata，
/// 也就是"确实从盘上拿掉的量"，不是删除前的目录估算。
#[derive(Serialize, Default, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CleanReport {
    /// 删掉的文件数（孤儿暂存按一个目录计一项，不摊到它内部的几百个文件）
    pub items: usize,
    pub bytes: u64,
    /// 删不动的（被占用/权限）：非 0 时前端要外显，别让人以为清干净了
    pub failed: usize,
}

/// 资源管理器/`openPath` 侧的目录串：Windows 上把正斜杠统一成反斜杠。
/// Rust 自己的 IO 两种斜杠都吃，所以这条只在把路径交给系统前用一次。
pub fn native_path(s: &str) -> String {
    if cfg!(windows) {
        s.replace('/', "\\")
    } else {
        s.to_string()
    }
}

/// 老版本把默认目录写死在用户目录下（`~\SideShift\{output,cache}`），那份绝对路径会被
/// settings.json 固化，升级后再也跟不到新的预选值。只改写「恰好等于旧默认」的字段——
/// 那是我们自己写进去的，不是用户挑的；用户手打或选过的路径一律不动。
pub fn unstick_legacy(dir: &str, legacy: &str, modern: &str) -> String {
    if dir == legacy {
        modern.to_string()
    } else {
        dir.to_string()
    }
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
    /// 默认目录对挂在给定数据根下（`{根}\SideShift\{output,cache}` 的布局唯一真源）。
    /// 数据根本身怎么挑见 `core::data_root::suggested_root`（便携 → 安装器指定 → 预选非系统盘 → 用户目录）
    pub fn defaults_in(root: &std::path::Path) -> Self {
        // 子目录名与安装壳显示的是同一份布局（`data_root::layout_in`），这里不重复写字面量。
        // 一段一段 join：写成 join("SideShift/output") 在 Windows 上会得到
        // `C:\Users\you\SideShift/output` 这种混合分隔符，见 native_path 的说明
        let (output, cache) = crate::core::data_root::layout_in(root);
        Self {
            output_dir: native_path(&output.display().to_string()),
            cache_dir: native_path(&cache.display().to_string()),
            strip_client_only: true,
            verify_after_build: false,
            download_source: DownloadSource::Official,
            concurrency: 6,
            auto_classify_online: true,
        }
    }

    /// 读写两端各过一遍：两个目录字段统一成本机分隔符（见 `native_path`），
    /// 无效下载源归位官方（否则设置页的下拉会显示成一个不存在的选项）
    pub fn normalized(mut self) -> Self {
        self.output_dir = native_path(&self.output_dir);
        self.cache_dir = native_path(&self.cache_dir);
        self.download_source = self.download_source.normalized();
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_use_native_separators() {
        let home = std::path::Path::new(if cfg!(windows) {
            "C:\\Users\\ban"
        } else {
            "/home/ban"
        });
        let s = AppSettings::defaults_in(&crate::core::data_root::suggested_root(home, home));
        assert!(!s.output_dir.contains('/'), "输出目录残留正斜杠: {}", s.output_dir);
        assert!(!s.cache_dir.contains('/'), "缓存目录残留正斜杠: {}", s.cache_dir);
        // 数据根与 home 回落共用同一套布局（引导页档位与默认值必须对得上）
        let in_home = AppSettings::defaults_in(&home.join("SideShift"));
        assert!(in_home.output_dir.ends_with("output"));
        assert!(in_home.cache_dir.ends_with("cache"));
        assert_eq!(
            AppSettings {
                output_dir: "D:/mc/out".into(),
                ..Default::default()
            }
            .normalized()
            .output_dir,
            if cfg!(windows) { "D:\\mc\\out" } else { "D:/mc/out" }
        );
    }

    /// 一次性迁移只能命中「我们自己写进去的旧默认」：用户挑过/手打的路径差一个字符都不能动，
    /// 否则就是把别人的服务器目录搬走了。
    #[test]
    fn unstick_legacy_rewrites_only_the_old_default() {
        assert_eq!(
            unstick_legacy("C:\\Users\\ban\\SideShift\\output", "C:\\Users\\ban\\SideShift\\output", "E:\\SideShift\\output"),
            "E:\\SideShift\\output"
        );
        for kept in [
            "D:\\mc\\out",                                   // 用户自己挑的盘
            "C:\\Users\\ban\\SideShift\\output\\",           // 旧默认多个分隔符
            "C:\\Users\\ban\\.minecraft\\downloads",         // 另一个已有目录
            "",                                              // 空字段由 load_settings 回落，不归这里管
        ] {
            assert_eq!(
                unstick_legacy(kept, "C:\\Users\\ban\\SideShift\\output", "E:\\SideShift\\output"),
                kept
            );
        }
    }

    /// 方案快照三段往返（下发前端 → 回传 start_conversion → 落盘回灌）必须留住端证据字段：
    /// 端标签不落盘、由 `client_side`/`server_side` 在渲染期反推，这里掉一个字段，
    /// 任务详情的「方案」签就会把整批模组错标成「未判定」。
    #[test]
    fn plan_roundtrip_keeps_env_evidence() {
        use std::collections::HashMap;
        let row = PlanMod {
            id: "sodium-fabric".into(),
            name: "Sodium".into(),
            version: "0.5.8".into(),
            loader: Some("Fabric".into()),
            disposition: ModDisposition::Keep,
            client_only: false,
            needs_review: false,
            auto_supplement: false,
            size_bytes: 1234,
            needs_download: false,
            local_path: None,
            pinned: None,
            depends: vec!["api".into()],
            src_path: Some("deps/mods/sodium.jar".into()),
            env_source: EnvSource::JarMetadata,
            env_conflict: true,
            client_side: Some(SideFlag::Required),
            server_side: Some(SideFlag::Optional),
            bytecode_hint: Some(BytecodeHint::ServerCode),
        };
        let to_frontend: Vec<PlanMod> =
            serde_json::from_value(serde_json::to_value([&row]).unwrap()).unwrap();
        let disk = serde_json::to_value(HashMap::from([("t1".to_string(), to_frontend)])).unwrap();
        let revived: HashMap<String, Vec<PlanMod>> = serde_json::from_value(disk).unwrap();
        let r = &revived["t1"][0];
        assert_eq!(r.client_side, Some(SideFlag::Required));
        assert_eq!(r.server_side, Some(SideFlag::Optional));
        assert_eq!(r.env_source, EnvSource::JarMetadata);
        assert!(r.env_conflict);
        assert_eq!(r.bytecode_hint, Some(BytecodeHint::ServerCode));
        assert_eq!(r.src_path.as_deref(), Some("deps/mods/sodium.jar"));
        assert_eq!(r.depends, vec!["api".to_string()]);
    }

    /// 报告新增字段必须能吃下旧存档：tasks.json 里的历史报告没有这些键，
    /// 一旦反序列化失败整个存档都会被当作损坏丢掉（用户看到的是「任务全没了」）
    #[test]
    fn legacy_report_json_still_loads_with_defaults() {
        let mut v = serde_json::to_value(ConversionReport {
            task_id: "t1".into(),
            output_file_name: "a-server.zip".into(),
            output_size_bytes: 1024,
            duration_sec: 30,
            removed: 3,
            kept: 9,
            added: 1,
            pending_review: vec!["X".into()],
            options: ConversionOptions::default(),
            file_count: 42,
            generated_files: vec!["start.bat".into()],
            start_jar: Some("fabric-server-launch.jar".into()),
            checks: Vec::new(),
        })
        .unwrap();
        let obj = v.as_object_mut().unwrap();
        obj.remove("fileCount");
        obj.remove("generatedFiles");
        obj.remove("startJar");
        obj.remove("checks");
        let old: ConversionReport = serde_json::from_value(v).unwrap();
        assert_eq!((old.file_count, old.start_jar), (0, None));
        assert!(old.generated_files.is_empty());
        assert!(old.checks.is_empty());
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
