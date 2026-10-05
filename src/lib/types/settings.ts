/** 应用设置（对应 Settings 屏 + 全局默认值） */

/** 下载源（Settings · 网络）：官方 / BMCLAPI 国内镜像，镜像不通自动回落官方 */
export type DownloadSource = "official" | "bmclapi";

/** 更新渠道（Settings · 外观与关于）：正式版 / Beta，对应 GitHub release 的 prerelease 标志 */
export type UpdateChannel = "stable" | "beta";

/** 端信息反查源（Settings · 端信息反查）：不发这一轮 / Modrinth 官方 / 麦块 API（国内快照镜像） */
export type EnvLookupSource = "off" | "official" | "minekuai";

/**
 * 界面语言档位（Settings · 外观与关于；Rust: AppSettings.locale，serde camelCase 同源）。
 *
 * `auto` 是一档真语义：**跟随系统语言**，不是"还没选过"。它永远不会成为生效档
 * （生效档只有三种，解析见 `@/lib/i18n/detection`），所以后端拿不到 `auto` 之外的歧义。
 * 三种语言的自称（简体中文 / 繁體中文 / English）按行业惯例不翻，用户在自己的语言里才认得出。
 */
export type AppLocale = "auto" | "zhCn" | "zhTw" | "enUs";

export interface AppSettings {
    /** 服务端输出目录（报告页输出路径） */
    outputDir: string;
    /** 工作缓存目录：解析与构建的临时文件存放位置 */
    cacheDir: string;
    /** 剔除客户端专属资源（光影/小地图/键鼠等） */
    stripClientOnly: boolean;
    /** 构建后自检：打包完成时对产物离线对账（模组/jar/依赖/启动件/包根/保留内容），不起服务端进程 */
    verifyAfterBuild: boolean;
    /** 下载源：版本表与加载器 jar 的镜像档位（模组文件与端信息反查都在 Modrinth，这一档管不到它们） */
    downloadSource: DownloadSource;
    /** 并发下载数 1–16（只管模组/服务端文件的并行下载；端信息反查用的是 Rust 侧固定的并发） */
    concurrency: number;
    /**
     * 端信息反查那一轮「问谁」（Rust: `env_lookup_source`）：关闭 / Modrinth 官方 / 麦块 API。
     * 三档互斥，所以是一枚下拉：原来「联网反查」总闸与「反查走镜像」开关管的是同一条链，
     * 分开摆就能出现「总闸关了、子项还亮着」这种界面自己答不出自己该问谁的状态。
     * 默认 `official`。`minekuai` ⇒ 那一轮**只发麦块**，官方三条腿（含它没有对应端点的
     * sha1 批量那条）一条都不发，存活自查不过就如实报「未全部完成」；
     * `off` ⇒ 零请求，只剩包内自证 + 本地索引 + 名称兜底。
     * （CF 构建标签 / 指纹腿独立于本档：问的是 CurseForge、凭用户自己的 Key。）
     */
    envLookupSource: EnvLookupSource;
    /**
     * 端判定的百科补全腿（MC百科词条的「运行环境」，Rust: `env_lookup_mcmod`）。
     * **默认关**：它不是官方行为——没有公开 API，靠解析两页 HTML，对方一次改版或一次
     * 人机验证就能让整条腿哑掉；结论也是社区编辑的第二手声明。开着时它只补平台各腿
     * （官方三条 / 麦块两条 + CF 构建/指纹）全答不上的那几行，且名字严格同形才采信。
     * `envLookupSource === "off"` 时它没有生效对象（那一轮压根不发），界面跟着灰掉。
     */
    envLookupMcmod: boolean;
    /**
     * 更新渠道。`null` 是有意义的一档：**跟随这一枚包自己的版本号**——
     * 版本号带预发布位的包收 Beta，纯版本号收正式版。在设置里选过一次就变成显式值。
     */
    updateChannel: UpdateChannel | null;
    /**
     * Modrinth 的 API 查询优先走 mcimirror（`mod.mcimirror.top`）、官方自动兜底（Rust: `modrinth_mirror`）。
     * **默认开**：透明反向代理，没有快照正确性风险；唯一风险是可用性，官方兜底消化它。
     * CurseForge 的数据**始终**经 mcimirror 获取（免 Key，不受本档控制）。
     * 生效范围是**取数与查询**（网络添加的搜索/详情/版本/译文、构建时的 Modrinth 请求）；
     * 端信息反查那一轮不看它——那一轮的源由 `envLookupSource` 定。
     */
    modrinthMirror: boolean;
    /**
     * 本机执行 loader installer（Forge / NeoForge 产物「上传即跑」的前提：装出 `libraries/` 与服务端本体）。
     * 默认开：这一档就是主路径。代价是多跑一次安装器（磁盘 + 时间），且本机没有合适 JDK 时任务直接失败。
     */
    installLoaderLocally: boolean;
    /** 装出来的 loader 留在 `{cacheDir}/installs/` 供后续任务复用（默认开）；关 = 每次现装现丢 */
    reuseLoaderInstalls: boolean;
    /**
     * 界面语言。**生效判定在前端**（翻译目录打进 bundle，见 `@/lib/i18n`），这里只是那一份持久化选择，
     * 所以切换语言不需要重启，也不需要后端参与翻译。缺省 `auto` = 跟随系统。
     */
    locale: AppLocale;
}

/**
 * 缓存占用报表（Rust: cache_usage）。计数来自一次真实目录扫描，
 * 与「清理」用的是同一套分类判据——界面说的数和实际回收的量必须对得上。
 */
export interface CacheUsage {
    /** 缓存目录绝对路径，与设置里的 cacheDir 同源 */
    cacheDir: string;
    /** 目录不存在 = 还没下载过任何东西，此时所有计数为 0，不是错误 */
    exists: boolean;
    /** 可复用的下载缓存（跨任务共享，删了要重新联网下） */
    filesCount: number;
    filesBytes: number;
    /** 其中最后一次使用早于 staleDays 前的 */
    staleCount: number;
    staleBytes: number;
    /** 半截下载的临时文件；有任务在跑时不计（可能正被写着） */
    partsCount: number;
    partsBytes: number;
    /** 注册表里已无此任务的暂存目录：个数按目录算，不按里面的文件算 */
    orphanCount: number;
    orphanBytes: number;
    /** 应用更新的暂存（cache/update/{版本}）：个数按版本目录算；暂未单独外显，跟随后端报表走 */
    updateCount: number;
    updateBytes: number;
    /** loader 复用安装的条目（cache/installs/...）：一项 100–160 MB。
     *  清理走独立的「Loader 安装」档：它是复用资产不是垃圾，删了下次要整包重装 */
    installsCount: number;
    installsBytes: number;
    /** 空壳目录：零字节，但用户会当成「没清干净」，所以要外显也要能清 */
    emptyDirs: number;
    /** 有任务在排队或运行：「清空全部」禁用 */
    busy: boolean;
    /** 过期门槛（天）。后端常量，前端只读：显示的「30 天」与删除判据是同一个值 */
    staleDays: number;
}

/** 一次清理的真实结果：bytes 是删掉的每个文件删除前读到的字节合计，不是删除前的目录估算 */
export interface CleanReport {
    /** 删掉的条目数（一个孤儿暂存目录算一项，不摊成它内部的几百个文件） */
    items: number;
    bytes: number;
    /** 删不动的（被占用 / 权限）：非 0 时要外显，别让人以为清干净了 */
    failed: number;
}

/** 下载缓存的清理口径：只删过期 / 清空全部 */
export type CacheCleanMode = "stale" | "all";
