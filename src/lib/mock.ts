/**
 * Mock 数据源：后端 command 就绪前驱动全部 UI（数据与设计稿 SS.pen 逐屏对齐）。
 * Phase 2 Rust 侧实现真实 command 后，api.ts 门面自动切换，本文件仅供浏览器 dev 模式使用。
 */
import type {
    AppSettings,
    ConversionOptions,
    ConversionReport,
    ConversionTask,
    DownloadEstimate,
    ModSearchPage,
    ModSearchQuery,
    ModVersionEntry,
    PackDirNode,
    PackManifest,
    PlanMod,
    StartResult,
    TaskLogLine,
    VersionOption,
} from "./types";
import { outputNameOf } from "./format";

/* ---------------- 静态样例数据 ---------------- */

export const mockManifest: PackManifest = {
    fileName: "vault-hunters-2.4.1.mrpack",
    loader: "fabric",
    mcVersion: "1.20.1",
    modCount: 187,
    sizeBytes: 84 * 1024 * 1024,
    parsed: true,
};

/**
 * 浏览器 dev 下的解析模拟：文件名取自拖入/选择的路径，其余字段沿用 mockManifest；
 * 扩展名不在白名单内时返回 parsed:false，用于演示解析失败态。
 */
export function mockParsePack(path: string): PackManifest {
    const fileName = path.split(/[\\/]/).pop() || mockManifest.fileName;
    const supported = /\.(mrpack|zip|7z)$/i.test(fileName);
    return supported
        ? { ...mockManifest, fileName }
        : {
              ...mockManifest,
              fileName,
              parsed: false,
              error: `不支持的包格式：${fileName}（仅支持 .mrpack / .zip / .7z）`,
          };
}

/**
 * 模组处置方案全量（与设计稿计数逐一对齐：剔除 41 / 保留 146 / 新增 2 = 187 个客户端模组 + 2 个补齐依赖）。
 * 前三行剔除项与两行新增项为手工样例（对应 Convert 卡可见行），其余由名称池生成，
 * 让"查看全部 41 项剔除清单"弹窗、保留计数等 UI 都有真实体量的数据可渲染。
 */
const REMOVE_SAMPLES: PlanMod[] = [
    { id: "optifine", name: "OptiFine", version: "F9M2+1.20.1", disposition: "remove", clientOnly: true, needsReview: false, autoSupplement: false },
    { id: "xaeros-minimap", name: "Xaero's Minimap", version: "23.10.0", loader: "Fabric", disposition: "remove", clientOnly: true, needsReview: false, autoSupplement: false },
    { id: "viafabricplus", name: "ViaFabricPlus", version: "3.4.11", loader: "客户端/服务端两可用", disposition: "remove", clientOnly: false, needsReview: true, autoSupplement: false },
    { id: "geckolib", name: "GeckoLib", version: "4.4.7", loader: "Fabric", disposition: "remove", clientOnly: false, needsReview: true, autoSupplement: false },
];

const ADD_SAMPLES: PlanMod[] = [
    { id: "fabric-api", name: "Fabric API", version: "0.92.2+1.20.1", loader: "Fabric", disposition: "add", clientOnly: false, needsReview: false, autoSupplement: true },
    { id: "spark", name: "spark", version: "1.10.53", loader: "Fabric", disposition: "add", clientOnly: false, needsReview: false, autoSupplement: false },
];

/** 客户端专属模组名池（渲染/输入/小地图/HUD 类，服务端一律剔除） */
const CLIENT_ONLY_POOL = [
    "Sodium", "Iris Shaders", "Continuity", "CIT Resewn", "LambDynamicLights", "Dynamic FPS",
    "Mod Menu", "Inventory Profiles Next", "Mouse Tweaks", "Jade", "WTHIT", "Roughly Enough Items",
    "Enhanced Visuals", "AppleSkin", "Fabric Skyboxes", "Zoomify", "MiniHUD", "Item Scroller",
    "Litematica", "Tweakeroo", "MaLiLib", "Reese's Sodium Options", "Sodium Extra", "ImmediatelyFast",
    "Entity Culling", "Better Third Person", "Not Enough Animations", "Emotecraft", "First-person Model",
    "3D Skin Layers", "Sound Physics Remastered", "Ambient Sounds", "Presence Footsteps", "Better F3",
    "Debugify", "Borderless Mining", "Fullscreen Fixes", "Light Overlay",
];

/** 服务端可保留模组名池（性能/内容/前置库） */
const KEEP_POOL = [
    "Lithium", "FerriteCore", "ModernFix", "C2ME", "Krypton", "Fabric Language Kotlin",
    "Architectury API", "Cloth Config", "Create", "Botania", "Patchouli", "Applied Energistics 2",
    "Mekanism", "Immersive Engineering", "Sophisticated Backpacks", "Waystones", "Farmer's Delight",
    "Supplementaries", "Quark", "Terralith", "Incendium", "Nullscape", "YUNG's Better Mineshafts",
    "Iron Furnaces",
];

/** 生成保留清单：146 项（设计稿计数），名称循环取自 KEEP_POOL */
function buildKept(count: number): PlanMod[] {
    return Array.from({ length: count }, (_, i) => ({
        id: `keep-${i}`,
        name: KEEP_POOL[i % KEEP_POOL.length],
        version: `1.${i % 9}.${(i % 7) + 1}`,
        loader: "Fabric",
        disposition: "keep" as const,
        clientOnly: false,
        needsReview: false,
        autoSupplement: false,
    }));
}

export const mockPlanMods: PlanMod[] = [
    ...REMOVE_SAMPLES,
    ...CLIENT_ONLY_POOL.map<PlanMod>((name, i) => ({
        id: `client-${i}`,
        name,
        version: `${(i % 4) + 1}.${i % 12}.${i % 9}`,
        loader: "Fabric",
        disposition: "remove",
        clientOnly: true,
        needsReview: false,
        autoSupplement: false,
    })),
    ...buildKept(146),
    ...ADD_SAMPLES,
];

/** 补真实感体积与下载来源：keep 行约 1/4 视为包内直取（未声明 jar），其余联网下载 */
mockPlanMods.forEach((m, i) => {
    m.sizeBytes = (3 + ((i * 7) % 26)) * 1_000_000 + ((i * 131) % 900) * 1000;
    m.needsDownload = !(m.disposition === "keep" && i % 4 === 0);
});

/** 反向依赖演示数据：保留项依赖被剔除/新增的前置库 */
const MOCK_DEPENDS: Record<string, string[]> = {
    Create: ["fabric-api"],
    Botania: ["fabric-api"],
    "Applied Energistics 2": ["fabric-api"],
    "Sophisticated Backpacks": ["geckolib"],
    "Farmer's Delight": ["geckolib"],
};
for (const m of mockPlanMods) {
    if (MOCK_DEPENDS[m.name]) m.depends = MOCK_DEPENDS[m.name];
}

/** 剔除清单弹窗数据 = 方案中处置为 remove 的子集 */
export const mockExcludedMods: PlanMod[] = mockPlanMods.filter(
    (m) => m.disposition === "remove"
);

/** 方案计数（任务卡「转换方案」行）：由全量方案派生，避免与 Convert 页数字漂移 */
export const mockPlanCounts = {
    remove: mockPlanMods.filter((m) => m.disposition === "remove").length,
    keep: mockPlanMods.filter((m) => m.disposition === "keep").length,
    add: mockPlanMods.filter((m) => m.disposition === "add").length,
};

/** 下载量预估（浏览器 dev 兜底）：行聚合 + 加载器经验值；真实缓存扣减/HEAD 实测只在 Rust 侧 */
export async function mockEstimateDownload(
    plan: PlanMod[],
    options: ConversionOptions
): Promise<DownloadEstimate> {
    await new Promise((r) => setTimeout(r, 120));
    let downloadBytes = 0;
    let fromPackBytes = 0;
    for (const m of plan) {
        if (m.disposition === "remove") continue;
        if (m.needsDownload) downloadBytes += m.sizeBytes ?? 0;
        else fromPackBytes += m.sizeBytes ?? 0;
    }
    downloadBytes += options.loaderVersion ? (options.mcVersion === "1.20.1" ? 12_000_000 : 25_000_000) : 0;
    return { downloadBytes, fromPackBytes, complete: true };
}

/** MC 版本下拉 */
export const mockMcVersions: VersionOption[] = [
    { value: "1.21.1", label: "1.21.1", group: "更多版本" },
    { value: "1.20.1", label: "1.20.1", recommended: true, group: "推荐" },
    { value: "1.20.4", label: "1.20.4", group: "更多版本" },
    { value: "1.19.4", label: "1.19.4", group: "更多版本" },
    { value: "1.18.2", label: "1.18.2", group: "更多版本" },
];

/** Fabric loader 版本下拉 */
export const mockLoaderVersions: VersionOption[] = [
    { value: "0.15.3", label: "0.15.3", recommended: true },
    { value: "0.14.24", label: "0.14.24" },
    { value: "0.14.22", label: "0.14.22" },
];

/** Java 版本下拉 */
export const mockJavaVersions: VersionOption[] = [
    { value: "21", label: "Java 21", recommended: true },
    { value: "17", label: "Java 17" },
];

/** Online Add 搜索结果（四页数据，与设计稿行对齐） */
const searchPool = [
    { id: "krypton", name: "Krypton", description: "轻量级协议层优化，显著降低服务端网络开销", author: "modmuss50", downloads: 12_040_000 },
    { id: "c2me", name: "C2ME", description: "并发化区块生成与写入，提升跑图性能", author: "ishland", downloads: 3_820_000 },
    { id: "ferritecore", name: "FerriteCore", description: "内存占用优化，适合大型整合包", author: "malte0612", downloads: 9_150_000 },
    { id: "spark", name: "spark", description: "服务端性能分析器，火焰图与内存采样", author: "lucko", downloads: 15_600_000 },
    { id: "ksyxis", name: "Ksyxis", description: "跳过原版世界生成的无效区域加载", author: "Dreeya", downloads: 2_100_000 },
    { id: "moreculling", name: "More Culling", description: "更激进的实体与方块剔除，提高帧数", author: "fxmorin", downloads: 4_500_000 },
    { id: "noisemax", name: "Noisemax", description: "生物群系与噪声生成优化", author: "thegggg", downloads: 980_000 },
    { id: "voxelmap", name: "VoxelMap", description: "客户端小地图（服务端转换将剔除）", author: "wover", downloads: 6_700_000 },
];

export function mockSearch(query: ModSearchQuery): ModSearchPage {
    const text = query.text.trim().toLowerCase();
    const filtered = searchPool.filter(
        (m) => !text || m.name.toLowerCase().includes(text) || m.description.toLowerCase().includes(text)
    );
    const pageSize = 4;
    const start = (query.page - 1) * pageSize;
    return {
        source: query.source,
        total: 128, // 设计稿演示值：共 128 个结果
        page: query.page,
        pageSize,
        results: filtered.slice(start, start + pageSize).map((m) => ({
            ...m,
            iconUrl: undefined,
            source: query.source,
            compatible: true,
            alreadyAdded: m.id === "spark",
        })),
    };
}

/** Modrinth /tag/category 的真实模组类别（dev 浏览器离线兜底；Tauri 走接口） */
export const mockModCategories: string[] = [
    "adventure", "cursed", "decoration", "economy", "equipment", "food", "game-mechanics",
    "library", "magic", "management", "minigame", "mobs", "optimization", "social",
    "storage", "technology", "transportation", "utility", "worldgen",
];

/** Mod Detail：Krypton 的版本行（整行点击下载） */
export const mockModVersions: ModVersionEntry[] = [
    { id: "v-0.2.3", versionNumber: "0.2.3", mcVersion: "1.20.1", loader: "fabric", date: "2023-11-02", sizeBytes: 412_000, recommended: true, url: "https://cdn.modrinth.com/data/fabric-krypton/versions/v-0.2.3/krypton-0.2.3.jar", sha1: "a3d5f1c09b7e2d46c8a05e1f3b7d9c2e4a6c8e01", fileName: "krypton-0.2.3.jar" },
    { id: "v-0.2.2", versionNumber: "0.2.2", mcVersion: "1.20.1", loader: "fabric", date: "2023-08-19", sizeBytes: 410_500, recommended: false, url: "https://cdn.modrinth.com/data/fabric-krypton/versions/v-0.2.2/krypton-0.2.2.jar", sha1: "b4e6a2d10c8f3e57d9b16f2a4c8e0d3f5b7d9e02", fileName: "krypton-0.2.2.jar" },
    { id: "v-0.2.1", versionNumber: "0.2.1", mcVersion: "1.20", loader: "fabric", date: "2023-06-07", sizeBytes: 408_100, recommended: false, url: "https://cdn.modrinth.com/data/fabric-krypton/versions/v-0.2.1/krypton-0.2.1.jar", sha1: "c5f7b3e21d9a4f68e0c27a3b5d9f1e4a6c8e0f03", fileName: "krypton-0.2.1.jar" },
    { id: "v-0.2.0", versionNumber: "0.2.0", mcVersion: "1.19.4", loader: "fabric", date: "2023-02-14", sizeBytes: 402_900, recommended: false, url: "https://cdn.modrinth.com/data/fabric-krypton/versions/v-0.2.0/krypton-0.2.0.jar", fileName: "krypton-0.2.0.jar" },
];

export const mockDefaultOptions: ConversionOptions = {
    mcVersion: "1.20.1",
    loaderVersion: "0.15.3",
    javaVersion: "21",
    memoryMb: 6144,
    generateScripts: true,
    nogui: false,
    agreeEula: true,
    serverPort: 25565,
    motd: "A SideShift powered Minecraft server",
    maxPlayers: 20,
    gamemode: "survival",
    difficulty: "easy",
    onlineMode: true,
    levelSeed: "",
    useAikarFlags: false,
    extraJvmArgs: "",
    outputOverride: "",
    keepDirs: [],
};

/** 包内可保留目录树（客户端保留目录弹窗演示数据；fileCount 递归统计） */
export const mockPackDirs: PackDirNode[] = [
    {
        name: "config",
        fileCount: 138,
        children: [
            { name: "jei", fileCount: 6, children: [] },
            { name: "sombreros", fileCount: 84, children: [] },
        ],
    },
    {
        name: "kubejs",
        fileCount: 24,
        children: [
            { name: "client_scripts", fileCount: 5, children: [] },
            { name: "server_scripts", fileCount: 8, children: [] },
        ],
    },
    { name: "shaderpacks", fileCount: 6, children: [] },
    {
        name: "maps",
        fileCount: 3,
        children: [
            { name: "journey_map", fileCount: 2, children: [] },
        ],
    },
];

/* ---------------- 任务引擎：定时器模拟四阶段流水线 ---------------- */

/** 任务内存仓（页面刷新即重置——真实实现中由 Rust 持久化） */
const tasks = new Map<string, ConversionTask>();
const timers = new Map<string, ReturnType<typeof setInterval>>();
let seq = 0;

const stagePlan: Array<{ stage: ConversionTask["stage"]; until: number; logs: string[] }> = [
    { stage: "parser", until: 15, logs: ["读取清单 vault-hunters 2.4.1 · minecraft-1.20.1"] },
    {
        stage: "detector",
        until: 30,
        logs: [
            "包内内容：模组 187 个 · 其他文件 62 个 · 需联网补取 0 个",
            "方案确认：剔除 41 · 保留 146 · 新增 2",
            "剔除名单：Sodium、Iris、Xaero's Minimap…等 41 个",
        ],
    },
    {
        stage: "downloader",
        until: 82,
        logs: [
            "取件计划 146 项 · 需联网 24（≈128.0 MB）· 整合包 118 · 本地 0 · 缓存命中 4 · 并发 6",
            "复用缓存 fabric-api-0.92.2+1.20.1.jar · 1.2 MB",
            "联网获取 spark-1.10.60.jar · 2.4 MB",
            "取件 · 模组 · 118 个 · 86.0 MB",
        ],
    },
    {
        stage: "builder",
        until: 100,
        logs: [
            "生成包根文件：start.bat、start.sh、eula.txt、server.properties",
            "打包 vault-hunters-2.4.1-server.zip · 148 个文件 · 96.4 MB",
        ],
    },
];

function now(): string {
    return new Date().toTimeString().slice(0, 8);
}

/** 实时条样例文件名（轮着当「正在下载哪一个」，与 stagePlan 的下载日志同一批名字） */
const NET_SAMPLE = [
    "fabric-api-0.92.2+1.20.1.jar",
    "sodium-fabric-0.5.8+mc1.20.1.jar",
    "modmenu-7.2.2.jar",
    "spark-1.10.60.jar",
    "xaerominimap-23.9.5.jar",
];

/* ---------------- 历史样例任务 ----------------
 * 让任务列表、任务详情（失败 / 已取消）、转换报告、错误卡在浏览器 dev 下都能直接走查——
 * 只靠 mockStartTask 只能产出成功态。真实实现里这些记录由 Rust 持久化，故仅注入一次。
 */

const MIN = 60_000;

type SeedTask = Omit<ConversionTask, "options" | "counts" | "logs"> &
    Partial<Pick<ConversionTask, "options" | "counts" | "logs">>;

function seedTask(p: SeedTask): ConversionTask {
    return {
        ...p,
        options: p.options ?? mockDefaultOptions,
        counts: p.counts ?? mockPlanCounts,
        logs: p.logs ?? [],
    };
}

const HISTORY: ConversionTask[] = [
    seedTask({
        id: "seed-failed",
        pack: { ...mockManifest, fileName: "medieval-world-1.3.zip", loader: "forge", mcVersion: "1.16.5", modCount: 74, sizeBytes: 210 * 1024 * 1024 },
        options: { ...mockDefaultOptions, mcVersion: "1.16.5", loaderVersion: "36.2.39", javaVersion: "17" },
        status: "failed",
        stage: "downloader",
        progress: 46,
        downloaded: 34,
        total: 74,
        fetch: {
            files: 74,
            bytes: 208_000_000,
            netFiles: 40,
            netBytes: 164_000_000,
            packFiles: 32,
            localFiles: 0,
            cachedFiles: 2,
        },
        netDone: 20,
        doneBytes: 96_000_000,
        counts: { remove: 18, keep: 52, add: 4 },
        createdAt: Date.now() - 42 * MIN,
        startedAt: Date.now() - 42 * MIN,
        finishedAt: Date.now() - 37 * MIN,
        error: {
            stage: "downloader",
            title: "forge-installer 退出码 1",
            detail: "下载 maven.minecraftforge.net 超时（已重试 3 次），可在设置中切换镜像源",
            retryable: true,
            attempts: 3,
        },
        logs: [
            { time: "14:02:11", stage: "parser", message: "读取清单 medieval-world 1.3 · minecraft-1.16.5", level: "info" },
            { time: "14:02:19", stage: "detector", message: "发现 forge-36.2.39 · 74 个模组 · 18 个客户端专属已标记剔除", level: "info" },
            { time: "14:07:03", stage: "downloader", message: "forge-installer.jar 下载超时 · 第 3 次重试失败", level: "error" },
        ],
    }),
    seedTask({
        id: "seed-success-2",
        pack: { ...mockManifest, fileName: "create-fabric-1.21.4.mrpack", mcVersion: "1.21.4", modCount: 96, sizeBytes: 412 * 1024 * 1024 },
        options: { ...mockDefaultOptions, mcVersion: "1.21.4", loaderVersion: "0.16.9" },
        status: "success",
        stage: "builder",
        progress: 100,
        counts: { remove: 22, keep: 71, add: 3 },
        createdAt: Date.now() - 3 * 60 * MIN,
        startedAt: Date.now() - 3 * 60 * MIN,
        finishedAt: Date.now() - 3 * 60 * MIN + 5 * MIN + 8_000,
        outputFileName: "create-server-1.21.4.zip",
        outputSizeBytes: 412 * 1024 * 1024,
        logs: [
            { time: "11:20:41", stage: "parser", message: "读取清单 create-fabric 1.21.4 · minecraft-1.21.4", level: "info" },
            { time: "11:25:02", stage: "builder", message: "生成 start.sh / start.bat · 打包 create-server-1.21.4.zip", level: "info" },
        ],
    }),
    seedTask({
        id: "seed-cancelled",
        pack: { ...mockManifest, fileName: "all-the-mods-9-1.0.2.mrpack", modCount: 342, sizeBytes: 690 * 1024 * 1024 },
        status: "cancelled",
        stage: "downloader",
        progress: 38,
        downloaded: 130,
        total: 342,
        fetch: {
            files: 342,
            bytes: 690_000_000,
            netFiles: 96,
            netBytes: 402_000_000,
            packFiles: 240,
            localFiles: 0,
            cachedFiles: 6,
        },
        netDone: 44,
        doneBytes: 268_000_000,
        counts: { remove: 96, keep: 240, add: 6 },
        createdAt: Date.now() - 26 * 60 * MIN,
        startedAt: Date.now() - 26 * 60 * MIN,
        finishedAt: Date.now() - 22 * 60 * MIN,
        logs: [
            { time: "13:41:08", stage: "detector", message: "发现 fabric-loader 0.15.3 · 342 个模组 · 96 个客户端专属已标记剔除", level: "info" },
            { time: "13:45:52", stage: "downloader", message: "任务已被用户取消", level: "warn" },
        ],
    }),
    seedTask({
        id: "seed-success-1",
        pack: { ...mockManifest, fileName: "fabulously-optimized-7.1.mrpack", modCount: 214, sizeBytes: 188 * 1024 * 1024 },
        status: "success",
        stage: "builder",
        progress: 100,
        counts: { remove: 63, keep: 123, add: 28 },
        createdAt: Date.now() - 5 * 60 * MIN,
        startedAt: Date.now() - 5 * 60 * MIN,
        finishedAt: Date.now() - 5 * 60 * MIN + 3 * MIN + 42_000,
        outputFileName: "fo-7.1-server.zip",
        outputSizeBytes: 236 * 1024 * 1024,
        logs: [
            { time: "09:12:30", stage: "parser", message: "读取清单 fabulously-optimized 7.1 · minecraft-1.20.1", level: "info" },
            { time: "09:16:12", stage: "builder", message: "生成 start.sh / start.bat · 打包 fo-7.1-server.zip", level: "info" },
        ],
    }),
];

let seeded = false;

/** 首次访问任务仓时注入历史样例（之后由用户操作驱动，删完不会再冒出来） */
function ensureSeeded(): void {
    if (seeded) return;
    seeded = true;
    for (const t of HISTORY) tasks.set(t.id, t);
    seq = HISTORY.length;
}

/** 启动（或重启）一个模拟转换任务；同一时间只跑一条，有任务在跑则排队 */
export function mockStartTask(
    options: ConversionOptions,
    pack: PackManifest = mockManifest
): StartResult {
    ensureSeeded();
    const queued = [...tasks.values()].some((t) => t.status === "running");
    const id = `task-${++seq}`;
    const task: ConversionTask = {
        id,
        pack,
        options,
        status: queued ? "queued" : "running",
        progress: 0,
        counts: mockPlanCounts,
        createdAt: Date.now(),
        startedAt: queued ? undefined : Date.now(),
        logs: [],
    };
    tasks.set(id, task);
    if (!queued) advance(id);
    return { taskId: id, queued };
}

/** 队首转正：当前无运行时，把最早创建的排队任务拉起（与 Rust release_and_next 同语义） */
function dequeueNext(): void {
    if ([...tasks.values()].some((t) => t.status === "running")) return;
    const next = [...tasks.values()]
        .filter((t) => t.status === "queued")
        .sort((a, b) => a.createdAt - b.createdAt)[0];
    if (!next) return;
    next.status = "running";
    next.startedAt = Date.now();
    advance(next.id);
}

function advance(id: string) {
    const timer = setInterval(() => {
        const task = tasks.get(id);
        if (!task || task.status !== "running") {
            clearInterval(timer);
            timers.delete(id);
            dequeueNext();
            return;
        }
        task.progress = Math.min(100, task.progress + 2);
        const seg = stagePlan.find((s) => task.progress <= s.until)!;
        task.stage = seg.stage;
        if (seg.stage === "downloader") {
            // 取件构成：与真实后端同一形态（146 项里只有 24 项真联网）
            task.fetch ??= {
                files: 146,
                bytes: 412_000_000,
                netFiles: 24,
                netBytes: 128_000_000,
                packFiles: 118,
                localFiles: 0,
                cachedFiles: 4,
            };
            const ratio = task.progress / 100;
            task.downloaded = Math.round(ratio * 146);
            task.total = 146;
            task.netDone = Math.round(ratio * task.fetch.netFiles);
            task.doneBytes = Math.round(ratio * task.fetch.bytes);
            // 实时条走联网口径（缓存/本地件零流量，计进来会让条跑得比真实网络快）
            task.activity = {
                kind: "net",
                subject: NET_SAMPLE[(task.netDone ?? 0) % NET_SAMPLE.length],
                doneBytes: Math.round(ratio * task.fetch.netBytes),
                totalBytes: task.fetch.netBytes,
                itemsDone: task.netDone ?? 0,
                itemsTotal: task.fetch.netFiles,
                rateBps: 2_400_000,
                attempt: 1,
            };
        } else if (seg.stage === "builder") {
            // 打包段 82→100：分母用产物体积，subject 随已写字节换目录
            const p = (task.progress - 82) / 18;
            const bytes = 101_187_000;
            task.activity = {
                kind: "zip",
                subject: p < 0.82 ? "模组" : p < 0.94 ? "config" : "根文件",
                doneBytes: Math.round(p * bytes),
                totalBytes: bytes,
                itemsDone: Math.round(p * 148),
                itemsTotal: 148,
                rateBps: 64_000_000,
                attempt: 1,
            };
        } else {
            task.activity = undefined;
        }
        // 每进入新阶段补一条日志（近似：按进度里程碑）
        if (task.progress % 15 === 2) {
            const line: TaskLogLine = {
                time: now(),
                stage: seg.stage!,
                message: seg.logs[Math.floor(Math.random() * seg.logs.length)],
                level: "info",
            };
            task.logs.push(line);
        }
        if (task.progress >= 100) {
            task.status = "success";
            task.activity = undefined;
            task.finishedAt = Date.now();
            task.outputFileName = outputNameOf(task.pack.fileName);
            task.outputSizeBytes = 96 * 1024 * 1024;
            clearInterval(timer);
            timers.delete(id);
            dequeueNext();
        }
    }, 200);
    timers.set(id, timer);
}

export function mockListTasks(): ConversionTask[] {
    ensureSeeded();
    return [...tasks.values()].sort((a, b) => b.createdAt - a.createdAt);
}

export function mockGetTask(id: string): ConversionTask | undefined {
    ensureSeeded();
    return tasks.get(id);
}

export function mockCancelTask(id: string): void {
    const task = tasks.get(id);
    if (task && (task.status === "running" || task.status === "queued")) {
        const wasQueued = task.status === "queued";
        task.status = "cancelled";
        task.activity = undefined;
        task.finishedAt = Date.now();
        task.logs.push({ time: now(), stage: task.stage ?? "builder", message: "任务已被用户取消", level: "warn" });
        // 排队行没有推进器，取消后由其替运行中任务交棒；运行中的交棒在 advance 里做
        if (wasQueued) dequeueNext();
    }
}

export function mockRetryTask(id: string): StartResult | undefined {
    const task = tasks.get(id);
    if (!task) return undefined;
    return mockStartTask(task.options, task.pack);
}

/** 从列表移除任务记录（终态任务才可删；运行中先取消） */
export function mockDeleteTask(id: string): void {
    const timer = timers.get(id);
    if (timer) clearInterval(timer);
    timers.delete(id);
    tasks.delete(id);
    dequeueNext();
}

export function mockReport(taskId: string): ConversionReport | undefined {
    ensureSeeded();
    const task = tasks.get(taskId);
    if (!task) return undefined;
    return {
        taskId,
        outputFileName: task.outputFileName ?? outputNameOf(task.pack.fileName),
        outputSizeBytes: task.outputSizeBytes ?? 0,
        durationSec: Math.round(((task.finishedAt ?? Date.now()) - (task.startedAt ?? Date.now())) / 1000),
        removed: task.counts?.remove ?? mockPlanCounts.remove,
        kept: task.counts?.keep ?? mockPlanCounts.keep,
        added: task.counts?.add ?? mockPlanCounts.add,
        pendingReview: ["ViaFabricPlus"],
        options: task.options,
    };
}

/* ---------------- 设置：localStorage 持久化 ---------------- */

const SETTINGS_KEY = "sideshift.settings";

export const mockDefaultSettings: AppSettings = {
    outputDir: "~/Documents/SideShift/output",
    cacheDir: "D:\\SideShift\\cache",
    stripClientOnly: true,
    verifyAfterBuild: false,
    downloadSource: "official",
    concurrency: 6,
};

/** 下载源下拉（Settings · 网络） */
export const mockDownloadSources: VersionOption[] = [
    { value: "official", label: "官方源 · Mojang + Forge", recommended: true },
    { value: "bmclapi", label: "BMCLAPI · 国内镜像" },
    { value: "github", label: "GitHub Releases" },
];

export function mockLoadSettings(): AppSettings {
    try {
        const raw = localStorage.getItem(SETTINGS_KEY);
        return raw ? { ...mockDefaultSettings, ...JSON.parse(raw) } : mockDefaultSettings;
    } catch {
        return mockDefaultSettings;
    }
}

export function mockSaveSettings(s: AppSettings): void {
    localStorage.setItem(SETTINGS_KEY, JSON.stringify(s));
}
