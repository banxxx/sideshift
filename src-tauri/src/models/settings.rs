use serde::{Deserialize, Serialize};

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

/// 端信息反查那一轮「问谁」（Settings · 端信息反查）。三档互斥，所以收成一枚下拉：
/// 原来 `Off` 是独立的「联网反查」总闸、`Minekuai` 是「反查走镜像」开关，两个控件管同一条链，
/// 于是能出现「总闸关了、子项还亮着」这种界面自己都不知道答案的状态
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum EnvLookupSource {
    /// 不发联网反查：只剩包内自证 + 本地索引 + 名称兜底
    Off,
    /// Modrinth 官方那三条腿（默认）
    #[default]
    Official,
    /// 麦块开放 API 的 Modrinth 项目快照。选中它时那一轮**只发麦块**、不悄悄回落官方
    Minekuai,
    /// 手改 settings.json 写进无效值时认成占位，不能让整份设置因为一个坏枚举全丢（同 DownloadSource）
    #[serde(other)]
    Unspecified,
}

impl EnvLookupSource {
    pub fn is_off(self) -> bool {
        self == Self::Off
    }

    pub fn is_minekuai(self) -> bool {
        self == Self::Minekuai
    }

    /// 占位值归位官方：写坏了来源时「多问一次官方」比「整轮不发」保守
    pub fn normalized(self) -> Self {
        match self {
            Self::Off | Self::Minekuai => self,
            _ => Self::Official,
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
    /// 端信息反查那一轮问谁（关闭 / Modrinth 官方 / 麦块 API），三档语义见 `EnvLookupSource`。
    /// 旧 settings.json 无此字段 → `Official`，不能让缺一个字段把整份设置抹掉
    #[serde(default)]
    pub env_lookup_source: EnvLookupSource,
    /// 端判定的百科补全腿（MC百科词条的「运行环境」）。
    /// **默认关**：它不是官方行为——没有公开 API，靠解析两页 HTML（搜索页 + 词条页），
    /// 对方一次改版、一次人机验证就能让整条腿哑掉；结论也是社区编辑的第二手声明。
    /// 它是**补全**而不是选源：只在 `env_lookup_source` 那一档的平台各腿（官方三条 / 麦块两条）
    /// 加 CF 构建/指纹全答不上的行才发请求，且名字严格同形才采信（见 `env::index::resolve_via_mcmod`）。
    /// 反查关掉（`Off`）时这一档无从生效，设置页要跟着灰掉。
    #[serde(default)]
    pub env_lookup_mcmod: bool,
    /// 更新渠道。`None` 不是"没选过"的临时状态而是真语义：**跟随这一枚包自己的版本号**——
    /// 带预发布位的包收 Beta，纯版本号收正式版，所以新装用户一个 setting 都没动也不会站错队。
    /// 用户在设置页选过一次之后就是显式值，从此不再看自己的版本号（这正是他要的手动切换）。
    #[serde(default)]
    pub update_channel: Option<UpdateChannel>,
    /// Modrinth 的 API 查询优先走 mcimirror（`mod.mcimirror.top`）、官方自动兜底。
    /// **默认开**：它是透明反向代理（同一份数据原样转发，没有快照正确性风险），唯一风险是
    /// 可用性，而候选链的官方兜底把这一点也消化了。CurseForge 的数据**始终**经 mcimirror
    /// 获取（免 Key，那是无 Key 世界里唯一能应答的源），不受本档控制——见 `source::candidates`。
    /// 生效范围是**取数与查询**（网络添加模组的搜索/详情/版本/译文、构建时的 Modrinth 请求）；
    /// 端信息反查那一轮不看它——那一轮的源由 `env_lookup_source` 定
    /// （反查用的是默认 `Downloader`，镜像档位没挂上去）。
    /// 旧 settings.json 无此字段 → default_fn 补 true。
    #[serde(default = "default_modrinth_mirror")]
    pub modrinth_mirror: bool,
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

/// Modrinth 镜像默认开：见字段注释
fn default_modrinth_mirror() -> bool {
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
    /// 应用更新的暂存（`cache\update\{版本}`）：个数按**版本目录**算，一个版本 = 安装包 + 它的签名
    pub update_count: usize,
    pub update_bytes: u64,
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
            env_lookup_source: EnvLookupSource::Official,
            // 百科补全腿默认关：非官方行为（HTML 解析 + 社区二手声明），见字段注释
            env_lookup_mcmod: false,
            update_channel: None,
            // Modrinth 镜像默认开：透明代理无正确性风险，官方兜底消化可用性风险（见字段注释）
            modrinth_mirror: true,
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
        self.env_lookup_source = self.env_lookup_source.normalized();
        // 只在"选过"的时候归位；None 是"跟随当前构建"，不能被当成无效值顶成正式版
        self.update_channel = self.update_channel.map(UpdateChannel::normalized);
        // 语言档位写坏了顶成"跟随系统"：否则设置页的下拉会显示成一个不存在的选项，
        // 而且落盘时会把 "unspecified" 写回 settings.json（同 download_source 那一行）
        self.locale = self.locale.normalized();
        self
    }
}
