use serde::{Deserialize, Serialize};
use crate::models::ModSource;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ModDisposition {
    Remove,
    Keep,
    Add,
}

/// 一枚 CF 构建的**取链许可**（自动分类那一轮探一次、落进 `cf-files-index.json`）。
///
/// 为什么非要探、不能离线判定：CF 的 file 对象里没有任何「这个项目允许不允许 API 发放
/// 下载链」的字段——实测一个 495 行的官方导出包，被整项目 403 拒掉的那 6 行
/// `isAvailable=true`、`fileStatus=4`、`externalLink=""`，与正常行**一模一样**。
/// 只有真敲一次 `/download-url` 才知道，而这一次探测不下载任何字节（空 JSON + 一次 HEAD）
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CfLink {
    /// 官方 `/download-url` 给链：主路，唯一有成文文档的一条
    #[default]
    Unknown,
    /// 官方给链（探过了，正常）
    Official,
    /// 官方不放行，但内容分发站按编号推得出来（构建期走回落链，见 `curseforge_build_url`）
    Derived,
    /// 两条路都没有 ⇒ 构建期拿不到字节。**必须在构建前就报出来**，不能等构建到一半炸
    Unavailable,
}

/// 端信息的证据来源（前端据此显示「依据什么判定」）。
/// 可信度顺序见 `env::rank`：jar 自证 > 平台按构建 > 平台按项目 > 镜像项目 > 百科词条 > 整合包声明 > 名称启发
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
    /// MC百科词条的「运行环境」：社区编辑整理的第二手声明，只在平台各腿全答不上时才问它一次
    /// （见 `env::index::resolve_via_mcmod`）。名字严格同形才采信，所以它对错的是「有没有这条依据」
    Mcmod,
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
    /// CF 编号行：这一枚**两条取链路都拿不到字节**（官方不放行、内容分发站也没有）。
    /// true = 构建时必须显式跳过并写进报告，不许静默丢（闸门读的是它，不是构建期的意外）
    #[serde(default)]
    pub cf_blocked: bool,
    /// 整合包清单把这枚声明成 `required`（字段缺失按 required 处理：CF 官方导出恒带这一条）。
    /// 分档用：必选模组缺件与可选模组缺件不是一回事，前者的闸门更硬
    #[serde(default)]
    pub cf_required: bool,
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
    /// 这一包里**按编号声明**的 CF 行数（官方导出的 CF 包才有；带 jar 字节的民间包恒为 0）。
    /// 它是包的属性、不是某一轮的补取结果：名字可能早就被磁盘索引答过了，但取字节每一步都要
    /// `/download-url` ⇒ 界面那句「要去 CurseForge 取 N 个模组」按这个数说
    pub cf_rows: usize,
    /// 上面那些行存在、又没配 CurseForge API Key ⇒ **true**。不看补取成功与否：补名字能靠缓存免
    /// Key，取 jar 字节不能，所以热索引的包也一样要说这句（否则人要到点转换才撞拒绝）
    pub cf_needs_key: bool,
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
