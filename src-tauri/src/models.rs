//! 跨进程数据模型：字段名/取值必须与 src/lib/types.ts 契约逐一对齐。
//! serde 约定：结构体 camelCase，枚举小写；可选字段 Option + skip_serializing_if。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::l10n::Msg;

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
/// 可信度顺序见 `env::rank`：jar 自证 > 平台按构建 > 平台按项目 > 镜像项目 > 整合包声明 > 名称启发
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
    /// 国内镜像（麦块开放 API）的项目级声明：内容同 ModrinthProject，但它是第三方快照，
    /// 可能滞后 ⇒ 只排在官方项目层之下、打包者声明之上
    MirrorProject,
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
/// 用户在添加那一刻选定的构建（与版本行一一对应，保证方案显示版本 = 实际下载版本）
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PinnedVersion {
    /// Modrinth：CDN 永久直链，存档下来构建时照打。
    /// CurseForge：**空串** —— 它的文件链是带时效的签名 URL，落档等于埋一颗到点失效的雷，
    /// 所以构建时按 `mod_id + file_id` 现取一条新鲜的（见 `downloader::curseforge_download_url`）
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha1: Option<String>,
    pub file_name: String,
    /// 来源平台。老存档无此字段 → None = Modrinth 那条老路（不取链、直接用 url）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<ModSource>,
    /// CurseForge 的 file id（`source = Curseforge` 时有值；与方案行 id = mod id 配对定位文件）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_id: Option<String>,
}

impl PinnedVersion {
    /// 这一钉是不是「构建时还得向 CurseForge 要一次链接」：三个条件缺一条都不去要
    pub fn needs_curseforge_link(&self) -> bool {
        self.url.is_empty()
            && self.source == Some(ModSource::Curseforge)
            && self.file_id.is_some()
    }
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
    /// 本次的 **Java 需求线**（由 MC 版本推的那档，"17"）：只当筛子用，不是"要装哪版"。
    /// 跑 installer 用哪一枚由 `java_path` 决定；这一档进报告与回看，语义始终是"包要什么"。
    pub java_version: String,
    /// 手选跑 installer 的那枚 JDK 绝对路径；空 = 自动（本机第一枚够格的）。
    /// 换机/卸掉之后这一枚可能不在本机了：`core::java::probe` 认不到就退回自动，不会把转换钉死。
    pub java_path: String,
    pub memory_mb: u32,
    pub generate_scripts: bool,
    pub nogui: bool,
    /// 自动写入 `eula=true`（**默认开**：关掉做出的包首次一律拒启，那是"默认给用户一个跑不起来的包"）
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
    /// Aikar's flags：G1GC 调优参数组，拼入 start 脚本 JVM 参数（**默认开**：官方推荐档，
    /// 且只在 start 脚本里出现，用户手改 `-Xmx` 之外不会碰到它）
    pub use_aikar_flags: bool,
    /// 用户附加 JVM 参数（原样拼接）
    pub extra_jvm_args: String,
    /* ---- 单包输出覆写 ---- */
    /// 本次转换输出目录；空 = 用全局设置
    pub output_override: String,
    /* ---- 客户端保留目录 ---- */
    /// 需要原样带入服务端的包内目录：相对路径（任意层级，如 kubejs/client_scripts），按前缀匹配
    pub keep_dirs: Vec<String>,
    /// 需要原样带入服务端的包内**根级散文件**（如 `options.txt`）：按逻辑相对路径**精确全等**匹配。
    ///
    /// 与 `keep_dirs` 分成两个字段而不是混进一个字符串数组：拷贝/预估那两处要按条目形状分叉
    /// （目录走 `{path}/` 前缀、文件走全等），混在一起前端就得猜"这条到底是不是文件"。
    /// 根级文件天然没有父目录，所以这一档不会触发「勾子项要不要收掉父级」那条冲突。
    pub keep_files: Vec<String>,
    /* ---- 本机安装 Loader ---- */
    /// 本次转换是否在本机跑 loader installer（Forge / NeoForge 产物「上传即跑」的前提）。
    ///
    /// **显式 bool，不引入 `Option`/第三态**：建包时 `default_options()` 从全局取初值，用户改过就存自己那份。
    /// 于是重试与任务快照永远按快照走——不存在"跟随全局"那种会随设置漂移的语义（同一份方案隔几天
    /// 重跑做出不一样的包，比包本身有问题更难查）。老存档缺这个字段走容器级 `serde(default)` = `Default::default()`。
    pub install_loader_locally: bool,
}

impl Default for ConversionOptions {
    fn default() -> Self {
        Self {
            mc_version: String::new(),
            loader_version: String::new(),
            java_version: String::new(),
            java_path: String::new(),
            memory_mb: 4096,
            generate_scripts: true,
            nogui: true,
            agree_eula: true,
            server_port: 25565,
            motd: "A Minecraft server".into(),
            max_players: 20,
            gamemode: "survival".into(),
            difficulty: "easy".into(),
            online_mode: true,
            level_seed: String::new(),
            use_aikar_flags: true,
            extra_jvm_args: String::new(),
            output_override: String::new(),
            keep_dirs: Vec::new(),
            keep_files: Vec::new(),
            install_loader_locally: true,
        }
    }
}

/// 保留树里的一个文件条目（弹窗展示用；`keep_files` 的取值 = 它的逻辑相对路径）
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PackFileNode {
    /// 文件名（不含路径），如 options.txt
    pub name: String,
    /// 原始字节；0 = 未知（index 没给 fileSize 且 zip 条目也没测到）
    pub size_bytes: u64,
    /// 从包根起算的逻辑相对路径（已剥 overrides 壳），如 kubejs/client_scripts/keep.js
    pub path: String,
}

/// 包内可保留内容的整棵树（客户端保留目录弹窗数据源）
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct PackDirTree {
    /// 目录树（mods 与 resourcepacks 已在建树时跳过）
    pub dirs: Vec<PackDirNode>,
    /// 根级散文件（`options.txt`、`servers.dat` 这类）：不带目录段，过去连展示位都没有
    pub files: Vec<PackFileNode>,
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
    /// 该目录内字节数（递归，含子目录；用于「勾之前先看多大」）
    pub size_bytes: u64,
    /// **直属**文件（不含子目录里的），按名升序；只读展示用，勾选仍走目录前缀
    pub files: Vec<PackFileNode>,
    /// 子目录节点，名字升序
    pub children: Vec<PackDirNode>,
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
pub struct VersionOption {
    pub value: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
}

/// JDK 探测结果（Rust: probe_java）。转换页在点「开始转换」**之前**就把「本机跑 installer 跑不跑得起来」
/// 显示出来：可预见的失败不该等 30 秒下载走完才说。状态口径沿用 [`CheckStatus`]，
/// 前端那套 Pass/Warn/Fail 的配色与图标不用再分叉一份。
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct JavaProbe {
    pub status: CheckStatus,
    /// 本次**会用上**的那枚 java 的绝对路径（手选命中 = 手选那枚，否则 = 自动挑的）；None = 本机压根没找到
    pub java_path: Option<String>,
    /// 解析出的主版本（8/17/21/25…）；`java -version` 认不出格式时为 None
    pub major: Option<u32>,
    /// 本次转换的最低需求线（由 MC 版本推的那档）；没传需求时 None = 只报有什么、不判够不够
    pub required_major: Option<u32>,
    /// 本机扫到的全部 JDK，按 `JAVA_HOME` → PATH 的顺序：转换页那颗下拉的候选就是它
    pub installed: Vec<JavaInstall>,
    /// 传进来的「手选那枚」已经不在本机了（卸载、换盘符、换机），本次改用自动挑的那一枚
    pub selected_missing: bool,
    /// 一句话结论（带真实数字与落点），失败卡那一路直接显示
    pub detail: String,
}

/// 本机一枚可用（`java -version` 认得出来）的 JDK。转换页的下拉按它列候选，
/// `path` 是标识、`major` 是显示名 —— 同版本两枚时路径不同，只按版本号选不出唯一一枚。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct JavaInstall {
    pub path: String,
    pub major: u32,
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
    /// 平台 URL slug：Modrinth 的 `id` 本来就是它，CurseForge 另给一栏（`id` 是数字 mod id）。
    /// 只有 slug 能进麦块的 `detail/{slug}`，所以中文简介那条线吃这个而不是 `id`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
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

/// 手动添加那一行的取证结果（本地 jar 与在线构建共用；两侧支持度来自阶梯跑完的那一层）
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

/// 详情页「翻译」按钮那份中文译文（麦块镜像的 `detail/{slug}`，机器翻译件）。
/// 两个字段各有一档覆盖率：`description_zh` 头部实测基本全有，`title_zh` 只有五成上下，
/// 所以「有译文」的判据是**两者都空才算没有**，且调用方要能只拿到其中一个
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct ModTranslation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title_zh: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description_zh: Option<String>,
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
    /// 端信息反查走国内镜像（麦块开放 API 的 Modrinth 项目快照）而不是 Modrinth 官方。
    /// **默认关**：判据来自一个无 SLA 的第三方快照，覆盖率实测也不是满的（收录外的 slug 返回 404）。
    /// 开着时联网那一轮**只发麦块**：官方三条腿（含它没有对应端点的 sha1 批量那条）一条都不发，
    /// 存活自查不过就直接记「这一轮没跑完」，不再悄悄回落官方。
    #[serde(default)]
    pub env_lookup_mirror: bool,
    /// 更新渠道。`None` 不是"没选过"的临时状态而是真语义：**跟随这一枚包自己的版本号**——
    /// 带预发布位的包收 Beta，纯版本号收正式版，所以新装用户一个 setting 都没动也不会站错队。
    /// 用户在设置页选过一次之后就是显式值，从此不再看自己的版本号（这正是他要的手动切换）。
    #[serde(default)]
    pub update_channel: Option<UpdateChannel>,
    /// CurseForge Core API 的 `x-api-key`。**None / 空串 = 没配**：那一侧的搜索与构建列表整块不可用，
    /// 界面据此给「去获取 Key」的出口，而不是让用户对着一条 403 猜原因。
    /// 这是用户自己的凭据：只写在 settings.json（他本机数据根），不进日志、不进仓库。
    #[serde(default)]
    pub curseforge_api_key: Option<String>,
    /// 本机执行 loader installer（Forge / NeoForge 想要「上传即跑」的前提：装出 `libraries/` 与服务端本体）。
    ///
    /// **默认开**：这一档存在的目的就是「解压即开服」，默认关等于把主路径藏起来。
    /// 代价是转换时多跑一次安装器（磁盘 + 时间），且本机没有合适 JDK 时任务直接失败、不降级。
    /// 关掉时打包链路与旧产物逐字节一致，一行分支都不进。
    #[serde(default = "default_install_loader")]
    pub install_loader_locally: bool,
    /// 装出来的 loader 留在 `{cache_dir}/installs/{loader}/{mc}-{ver}/` 供后续任务复用（默认开）。
    /// 关掉 = 每次现装现丢，装在任务的临时目录里、打完包即删：省磁盘但每次都吃一遍下载。
    #[serde(default = "default_reuse_installs")]
    pub reuse_loader_installs: bool,
    /// 界面语言。翻译目录只有一份、住在前端（`src/lib/i18n/resources/`），
    /// 所以后端**只存这一档**：生效语言的判定与切换都在前端，改档也不需要重启。
    /// 旧 settings.json 无此字段 → `Auto`（跟随系统），不能因为多一个字段就丢用户设置。
    #[serde(default)]
    pub locale: AppLocale,
}

/// 界面语言档位（Settings · 外观与关于）。线上码 camelCase，与前端 `AppLocale` 逐字对齐。
///
/// `Auto` 是一档真语义（跟随系统），不是"还没选过"；它永远不会成为生效档
/// ——生效档只有三种，由前端按 `navigator` 的语言偏好解析。
/// `Unspecified` 只吃「手改 settings.json 写错值」这一种情况：`persist::load_settings` 是
/// **整份回落默认**，一个错别字不该把用户所有设置抹掉（同 DownloadSource / UpdateChannel）。
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum AppLocale {
    /// 跟随系统语言（默认）
    #[default]
    Auto,
    ZhCn,
    ZhTw,
    EnUs,
    #[serde(other)]
    Unspecified,
}

impl AppLocale {
    /// 落盘前把占位值归回跟随系统（读写两端各过一遍，见 `AppSettings::normalized`）
    pub fn normalized(self) -> Self {
        if self == Self::Unspecified {
            Self::Auto
        } else {
            self
        }
    }
}

/// 更新渠道（Settings · 外观与关于）：正式版 / Beta，对应 GitHub release 的 prerelease 标志
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum UpdateChannel {
    Stable,
    Beta,
    /// 手改 settings.json 写进无效值时认成占位，不能让整份设置因为一个坏枚举全丢（同 DownloadSource）
    #[serde(other)]
    Unspecified,
}

impl UpdateChannel {
    /// 只认 beta，其余（含占位值）落回正式版——订阅错了方向比订阅保守更糟
    pub fn normalized(self) -> Self {
        if self == Self::Beta {
            Self::Beta
        } else {
            Self::Stable
        }
    }
}

/// 检查更新的结果（Rust: check_update）。结论在 Rust 侧算，前端不再自己比字符串：
/// `1.0.0-beta.2` 与 `1.0.0-beta.10` 这种号，字符串比较一定比反。
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    /// 本地版本（取自 tauri 的 package_info，与前端注入的 __APP_VERSION__ 同一个源）
    pub current: String,
    /// 该渠道下最新的那条 release；仓库还没发过 release、或本渠道一条都没有时为 null
    pub latest: Option<String>,
    /// latest 严格新于 current 才算有更新：同版本、更老都不提示
    pub has_update: bool,
}

fn default_online_classify() -> bool {
    true
}

/// 复用安装缓存默认开：一次装好的 Forge 服务端 100–160 MB，重装的下载代价没人该反复付
fn default_reuse_installs() -> bool {
    true
}

/// 与 `AppSettings::default()` 里的初值同一颗布尔：旧 settings.json 没写过这一档时也算开，
/// 不然「默认为开」只对全新安装成立，老用户的默认值会静停在关
fn default_install_loader() -> bool {
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
            // 第三方镜像默认关：见字段注释（覆盖率与新鲜度都不由我们保证）
            env_lookup_mirror: false,
            update_channel: None,
            curseforge_api_key: None,
            install_loader_locally: true,
            reuse_loader_installs: true,
            locale: AppLocale::Auto,
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
        // 其余字段直接铺 `Default::default()`：两份字面量各写一遍，加设置时漏一份是迟早的事
        Self {
            output_dir: native_path(&output.display().to_string()),
            cache_dir: native_path(&cache.display().to_string()),
            ..Self::default()
        }
    }

    /// 读写两端各过一遍：两个目录字段统一成本机分隔符（见 `native_path`），
    /// 无效下载源归位官方（否则设置页的下拉会显示成一个不存在的选项）
    pub fn normalized(mut self) -> Self {
        self.output_dir = native_path(&self.output_dir);
        self.cache_dir = native_path(&self.cache_dir);
        self.download_source = self.download_source.normalized();
        // 只在"选过"的时候归位；None 是"跟随当前构建"，不能被当成无效值顶成正式版
        self.update_channel = self.update_channel.map(UpdateChannel::normalized);
        // 语言档位写坏了顶成"跟随系统"：否则设置页的下拉会显示成一个不存在的选项，
        // 而且落盘时会把 "unspecified" 写回 settings.json（同 download_source 那一行）
        self.locale = self.locale.normalized();
        // 凭据字段：粘贴时常带首尾空白，带着空格发出去的 403 用户读不懂，
        // 所以在这里一次归位；清空的串记成 None，让「没配」与「配了个空」是同一个状态
        self.curseforge_api_key = self
            .curseforge_api_key
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty());
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

    /// 老 settings.json 里没有 updateChannel：必须补成 `None`（= 跟随当前构建），
    /// 不能因为多一个字段就把用户整份设置丢掉；档位值写坏了也一样只能归位，不能连带失败。
    #[test]
    fn settings_without_update_channel_still_load() {
        let legacy = r#"{"outputDir":"o","cacheDir":"c","stripClientOnly":true,"verifyAfterBuild":false,
            "downloadSource":"official","concurrency":6,"autoClassifyOnline":true}"#;
        let s: AppSettings = serde_json::from_str(legacy).expect("旧设置应能加载");
        assert_eq!(s.update_channel, None);

        let broken = r#"{"outputDir":"o","cacheDir":"c","stripClientOnly":true,"verifyAfterBuild":false,
            "downloadSource":"official","concurrency":6,"updateChannel":"ntfs"}"#;
        let s: AppSettings = serde_json::from_str(broken).expect("无效渠道值不该拖垮整份设置");
        assert_eq!(s.normalized().update_channel, Some(UpdateChannel::Stable));
    }

    /// 老设置里没有这个字段：必须能加载（缺 Key = CurseForge 侧整块不可用，不是错误）
    #[test]
    fn settings_without_curseforge_key_still_load() {
        let legacy = r#"{"outputDir":"o","cacheDir":"c","stripClientOnly":true,"verifyAfterBuild":false,
            "downloadSource":"official","concurrency":6,"autoClassifyOnline":true}"#;
        let s: AppSettings = serde_json::from_str(legacy).expect("旧设置应能加载");
        assert_eq!(s.curseforge_api_key, None);
    }

    /// 旧 settings.json / 旧任务存档都没有装 Loader 那三颗开关：必须加载成功，且默认值不能反过来——
    /// 本机安装与复用都默认**开**（一次装好的服务端 100–160 MB，不该让老用户从此每次重下重装；
    /// 而「产物上传即开服」这条主路径也不该对老用户隐身）。关掉才等价于旧产物那份行为。
    #[test]
    fn loader_switch_defaults_hold_for_legacy_payloads() {
        let legacy = r#"{"outputDir":"o","cacheDir":"c","stripClientOnly":true,"verifyAfterBuild":false,
            "downloadSource":"official","concurrency":6,"autoClassifyOnline":true}"#;
        let s: AppSettings = serde_json::from_str(legacy).expect("旧设置应能加载");
        assert!(s.install_loader_locally, "本机安装默认必须开");
        assert!(s.reuse_loader_installs, "复用默认必须开");

        // 任务快照里的方案同理：缺字段 = 没表过态，跟默认档一起开，但不会跟着全局设置的当前值漂移
        let opts: ConversionOptions =
            serde_json::from_str(r#"{"mcVersion":"1.20.1","loaderVersion":"47.4.10"}"#)
                .expect("旧方案存档应能加载");
        assert!(opts.install_loader_locally);
        assert_eq!(opts.memory_mb, 4096, "其余字段走 Default，别悄悄改了老任务的档位");
    }

    /// 老设置里完全没有语言这一档（i18n 之前写的 settings.json）：必须加载成功并落 `Auto`，
    /// 也就是"跟随系统"。档位值写错（手改、或未来版本加了新档又被老程序读）只能归位成 Auto，
    /// 不能把整份设置连带丢掉——`persist::load_settings` 的回落粒度是整份文件。
    #[test]
    fn settings_without_locale_still_load_and_bad_value_normalizes() {
        let legacy = r#"{"outputDir":"o","cacheDir":"c","stripClientOnly":true,"verifyAfterBuild":false,
            "downloadSource":"official","concurrency":6,"autoClassifyOnline":true}"#;
        let s: AppSettings = serde_json::from_str(legacy).expect("旧设置应能加载");
        assert_eq!(s.locale, AppLocale::Auto, "缺字段必须落跟随系统");

        let pinned: AppSettings = serde_json::from_str(&format!(
            r#"{{"outputDir":"o","cacheDir":"c","stripClientOnly":true,"verifyAfterBuild":false,
            "downloadSource":"official","concurrency":6,"locale":"zhTw"}}"#
        ))
        .expect("显式语言档位应能加载");
        assert_eq!(pinned.locale, AppLocale::ZhTw);

        let broken = r#"{"outputDir":"o","cacheDir":"c","stripClientOnly":true,"verifyAfterBuild":false,
            "downloadSource":"official","concurrency":6,"locale":"zh_CN"}"#;
        let s: AppSettings = serde_json::from_str(broken).expect("无效语言值不该拖垮整份设置");
        assert_eq!(s.normalized().locale, AppLocale::Auto);
    }

    /// 粘贴进来的 Key 常带空白：带着空格发出去只会收到一条读不懂的 403，所以读写两端都归位；
    /// 全空串等于「没配」，界面才不会再显示一个空输入框当成已配置
    #[test]
    fn curseforge_key_trims_and_blank_becomes_none() {
        let s = AppSettings {
            curseforge_api_key: Some("  $23-abc:def  ".into()),
            ..Default::default()
        }
        .normalized();
        assert_eq!(s.curseforge_api_key.as_deref(), Some("$23-abc:def"));

        for blank in ["", "   ", "\t\n"] {
            let s = AppSettings {
                curseforge_api_key: Some(blank.into()),
                ..Default::default()
            }
            .normalized();
            assert_eq!(s.curseforge_api_key, None, "{blank:?} 应归位为未配置");
        }
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
            installed: true,
            checks: Vec::new(),
        })
        .unwrap();
        let obj = v.as_object_mut().unwrap();
        obj.remove("fileCount");
        obj.remove("generatedFiles");
        obj.remove("startJar");
        obj.remove("installed");
        obj.remove("checks");
        let old: ConversionReport = serde_json::from_value(v).unwrap();
        assert_eq!((old.file_count, old.start_jar), (0, None));
        assert!(old.generated_files.is_empty());
        // 旧存档没这一键 ⇒ 没本机装过（那条链路当时还不存在）
        assert!(!old.installed);
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
