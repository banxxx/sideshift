/**
 * SideShift 领域类型 + IPC 契约（前端 ↔ Rust 的唯一数据接口定义）
 *
 * 约定：
 *  - 所有跨进程载荷集中在此，Rust 侧 serde 序列化的字段名必须与之对齐（camelCase）。
 *  - 这些类型对应 SS.pen 各屏所需数据；Phase 2 实现 Rust command 时以其为契约。
 *  - 金额/体积单位统一：字节用 number（bytes），时长用秒（number），进度用 0-100 整数。
 */

/** 模组加载器类型 */
export type LoaderKind = "fabric" | "forge" | "neoforge";

/** 解析出的整合包元信息（对应 Home 详情卡 + Convert 运行环境） */
export interface PackManifest {
    /** 原始文件名，如 vault-hunters-2.4.1.mrpack */
    fileName: string;
    /** 加载器 */
    loader: LoaderKind;
    /** Minecraft 版本，如 1.20.1 */
    mcVersion: string;
    /** 客户端模组总数 */
    modCount: number;
    /** 包体积（字节） */
    sizeBytes: number;
    /** 解析是否成功；失败时 error 描述原因 */
    parsed: boolean;
    error?: string;
    /** 源包绝对路径（后端下发、创建任务时原样带回） */
    sourcePath?: string;
}

/** 单个模组在转换方案中的处置（对应 Convert 模组方案三行） */
export type ModDisposition = "remove" | "keep" | "add";

/** 转换方案里的一行模组 */
export interface PlanMod {
    id: string;
    name: string;
    /** 模组版本号 */
    version: string;
    /** 可选：兼容的加载器标注，如 Fabric */
    loader?: string;
    disposition: ModDisposition;
    /** 客户端专属（服务端需剔除） */
    clientOnly: boolean;
    /** 需人工确认（如 ViaFabricPlus） */
    needsReview: boolean;
    /** 自动补齐的依赖项（移除时给依赖警告） */
    autoSupplement: boolean;
    /** 「从本地添加」的 .jar 绝对路径（真实后端直接取本地文件） */
    localPath?: string;
    /** 依赖的其他方案行 id（mrpack depends 元数据；反向依赖警告用） */
    depends?: string[];
}

/** 转换可选项（对应 Convert 启动参数 + 运行环境 + 服务端设置） */
export interface ConversionOptions {
    /** 目标 Minecraft 版本 */
    mcVersion: string;
    /** 加载器版本，如 0.15.3 */
    loaderVersion: string;
    /** Java 版本，如 21 */
    javaVersion: string;
    /** 内存上限（MB） */
    memoryMb: number;
    /** 生成启动脚本 start.sh / start.bat */
    generateScripts: boolean;
    /** 无界面模式 --nogui */
    nogui: boolean;
    /** 自动写入 eula=true */
    agreeEula: boolean;
    /** 服务器端口（server.properties server-port） */
    serverPort: number;
    /** 服务器 MOTD */
    motd: string;
    /** 最大人数 */
    maxPlayers: number;
    /** 游戏模式 */
    gamemode: "survival" | "creative" | "adventure" | "spectator";
    /** 难度 */
    difficulty: "peaceful" | "easy" | "normal" | "hard";
    /** 正版验证 online-mode */
    onlineMode: boolean;
    /** 世界种子（空=随机） */
    levelSeed: string;
    /** Aikar's flags G1GC 调优参数组 */
    useAikarFlags: boolean;
    /** 附加 JVM 参数（原样拼入启动脚本） */
    extraJvmArgs: string;
    /** 本次输出目录覆写（空=用全局设置） */
    outputOverride: string;
    /** 原样带入服务端的包内目录：相对路径（任意层级），按前缀匹配拷贝 */
    keepDirs: string[];
}

/** 包内可保留目录树节点（客户端保留目录弹窗数据源） */
export interface PackDirNode {
    /** 目录名（不含路径），如 client_scripts */
    name: string;
    /** 该目录内文件数（递归，含子目录） */
    fileCount: number;
    /** 子目录节点，同层按名升序 */
    children: PackDirNode[];
}

/** 流水线四阶段（Shift Rail 站点，对应 Rust core 四模块） */
export type PipelineStage = "parser" | "detector" | "downloader" | "builder";

/** 任务状态 */
export type TaskStatus =
    | "queued"
    | "running"
    | "success"
    | "failed"
    | "cancelled";

/** 一条流水线日志（对应控制台时间轴行） */
export interface TaskLogLine {
    /** HH:mm:ss */
    time: string;
    stage: PipelineStage;
    message: string;
    level: "info" | "warn" | "error";
}

/** 转换任务（对应 Task 页 / Tasks 列表卡 / Report） */
export interface ConversionTask {
    id: string;
    pack: PackManifest;
    options: ConversionOptions;
    status: TaskStatus;
    /** 当前阶段；未开始时为空 */
    stage?: PipelineStage;
    /** 总进度 0-100 */
    progress: number;
    /** 下载阶段计数（其余阶段可空） */
    downloaded?: number;
    total?: number;
    /** 创建/开始/结束时间（epoch ms） */
    createdAt: number;
    startedAt?: number;
    finishedAt?: number;
    /** 失败原因（status=failed 时有值） */
    error?: TaskError;
    /** 转换方案计数（任务信息卡「转换方案」行、列表卡副标题） */
    counts?: { remove: number; keep: number; add: number };
    /** 输出文件名 / 体积（成功时） */
    outputFileName?: string;
    outputSizeBytes?: number;
    logs: TaskLogLine[];
}

/** 结构化错误载荷（对应 Errors 族四张卡） */
export interface TaskError {
    /** 出错阶段 */
    stage: PipelineStage;
    /** 人类可读标题，如「依赖下载失败」 */
    title: string;
    /** 详情 */
    detail: string;
    /** 是否可重试 */
    retryable: boolean;
    /** 已重试次数（下载失败用） */
    attempts?: number;
    /** 关联日志尾（构建失败用） */
    logTail?: string[];
    /** 进程退出码（构建失败用） */
    exitCode?: number;
}

/** 转换报告（对应 Report 屏） */
export interface ConversionReport {
    taskId: string;
    outputFileName: string;
    outputSizeBytes: number;
    /** 耗时（秒） */
    durationSec: number;
    /** 变更计数 */
    removed: number;
    kept: number;
    added: number;
    /** 待人工确认项 */
    pendingReview: string[];
    options: ConversionOptions;
}

/** 版本下拉项（对应 Convert · Version Dropdown） */
export interface VersionOption {
    value: string;
    label: string;
    /** 推荐项（列表顶部高亮 + check） */
    recommended?: boolean;
    group?: string;
}

/** 在线搜索的一个模组结果（对应 Online Add 行，模组级、无版本概念） */
export interface ModSearchResult {
    id: string;
    name: string;
    /** 一句话简介 */
    description: string;
    author: string;
    downloads: number;
    /** 图标 URL（可空则占位） */
    iconUrl?: string;
    source: "modrinth" | "curseforge";
    /** 当前 MC/加载器下是否已有可用版本 */
    compatible: boolean;
    /** 是否已在新增列表中 */
    alreadyAdded: boolean;
}

/** 某模组的一个可下载构建版本（对应 Mod Detail 版本行，整行点击下载） */
export interface ModVersionEntry {
    id: string;
    versionNumber: string;
    mcVersion: string;
    loader: LoaderKind;
    /** 发布时间 YYYY-MM-DD */
    date: string;
    sizeBytes: number;
    recommended: boolean;
}

/** 在线搜索结果分页（对应 Online Add 分页页脚） */
export interface ModSearchPage {
    source: "modrinth" | "curseforge";
    total: number;
    results: ModSearchResult[];
    /** 当前页（1 起） */
    page: number;
    pageSize: number;
}

/** 在线搜索筛选参数 */
export interface ModSearchQuery {
    source: "modrinth" | "curseforge";
    text: string;
    mcVersion: string;
    loader: LoaderKind;
    /** 类别过滤，可空=全部 */
    category?: string;
    page: number;
}

/** 下载源（Settings · 网络） */
export type DownloadSource = "official" | "bmclapi" | "github";

/** 应用设置（对应 Settings 屏 + 全局默认值） */
export interface AppSettings {
    /** 服务端输出目录（报告页输出路径） */
    outputDir: string;
    /** 工作缓存目录：解析与构建的临时文件存放位置 */
    cacheDir: string;
    /** 剔除客户端专属资源（光影/小地图/键鼠等） */
    stripClientOnly: boolean;
    /** 构建后自动校验：生成前空跑一次验证依赖完整性 */
    verifyAfterBuild: boolean;
    /** 下载源（Maven 镜像） */
    downloadSource: DownloadSource;
    /** 并发下载数 1–16 */
    concurrency: number;
}

/** 流水线进度事件载荷（Rust app.emit("conversion://progress", payload)） */
export interface ProgressEvent {
    taskId: string;
    stage: PipelineStage;
    progress: number;
    downloaded?: number;
    total?: number;
    log?: TaskLogLine;
}

/** 后端事件名常量（与 Rust emit 字符串保持一致） */
export const EVENTS = {
    progress: "conversion://progress",
    done: "conversion://done",
} as const;
