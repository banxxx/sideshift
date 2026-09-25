/**
 * Mock 静态夹具：后端 command 就绪前驱动全部 UI（数据与设计稿 SS.pen 逐屏对齐）。
 * 只放无状态数据与纯函数；流水线定时器在 tasks.ts，分类兜底在 classify.ts。
 */
import type {
    ConversionOptions,
    JavaProbe,
    ModSearchPage,
    ModSearchQuery,
    ModVersionEntry,
    PackDirNode,
    PackManifest,
    PlanMod,
    SideFlag,
    VersionOption,
} from "@/lib/types";

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
    { id: "optifine", name: "OptiFine", version: "F9M2+1.20.1", disposition: "remove", clientOnly: true, needsReview: false, autoSupplement: false, envSource: "mrpack", clientSide: "required", serverSide: "unsupported" },
    { id: "xaeros-minimap", name: "Xaero's Minimap", version: "23.10.0", loader: "Fabric", disposition: "remove", clientOnly: true, needsReview: false, autoSupplement: false, envSource: "modrinthHash", envConflict: true, clientSide: "required", serverSide: "optional" },
    { id: "viafabricplus", name: "ViaFabricPlus", version: "3.4.11", loader: "客户端/服务端两可用", disposition: "remove", clientOnly: false, needsReview: true, autoSupplement: false, envSource: "modrinthProject", clientSide: "required", serverSide: "optional" },
    { id: "geckolib", name: "GeckoLib", version: "4.4.7", loader: "Fabric", disposition: "remove", clientOnly: false, needsReview: true, autoSupplement: false, envSource: "jarMetadata", clientSide: "unsupported", serverSide: "required" },
];

const ADD_SAMPLES: PlanMod[] = [
    { id: "fabric-api", name: "Fabric API", version: "0.92.2+1.20.1", loader: "Fabric", disposition: "add", clientOnly: false, needsReview: false, autoSupplement: true, envSource: "jarMetadata", clientSide: "optional", serverSide: "required" },
    { id: "spark", name: "spark", version: "1.10.53", loader: "Fabric", disposition: "add", clientOnly: false, needsReview: false, autoSupplement: false, envSource: "modrinthProject", clientSide: "optional", serverSide: "optional" },
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

/** 保留行的两侧支持度：三档循环，让「两端必需 / 服务端必需 / 两端可选」在浏览器预览里都出现 */
function keepSides(i: number): { clientSide?: SideFlag; serverSide?: SideFlag } {
    if (i % 6 === 5) return {};
    return {
        clientSide: (i % 3 === 0 ? "required" : "optional") as SideFlag,
        serverSide: (i % 3 === 2 ? "optional" : "required") as SideFlag,
    };
}

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
        // 每 6 行留 1 行「无证据」，各演示一种字节码提示：serverCode 那行属于「名称层被 jar
        // 事实按住」的有依据保留；clientOnlyShape 那行经 mockClassify 落进剔除分组标待确认
        envSource: (i % 6 === 5 ? "unknown" : "jarMetadata") as PlanMod["envSource"],
        ...keepSides(i),
        bytecodeHint:
            i % 6 === 5 ? (i % 12 === 5 ? "serverCode" : "clientOnlyShape") : undefined,
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
        envSource: "mrpack",
        clientSide: "required",
        serverSide: "unsupported",
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

/** 本机 JDK 探测（浏览器 dev）：这台"假机器"装着 Java 21，够 17 与 21 两档需求，选 8 才出 fail 那一行 */
export function mockJavaProbe(requiredVersion: string | null): JavaProbe {
    const major = 21;
    const required = requiredVersion
        ? Number.parseInt(requiredVersion.replace(/\D/g, ""), 10)
        : Number.NaN;
    const need = Number.isNaN(required) ? null : required;
    const ok = need === null || major >= need;
    return {
        status: ok ? "pass" : "fail",
        javaPath: "C:\\Program Files\\Java\\jdk-21\\bin\\java.exe",
        major,
        requiredMajor: need,
        detail: ok
            ? `已检测到 Java ${major}${need ? `（本次需要 Java ${need} 及以上）` : ""}`
            : `本机 Java ${major} 低于本次需要的 Java ${need}，转换会在这里失败`,
    };
}

/** Online Add 搜索结果（四页数据，与设计稿行对齐；两侧支持度按 Modrinth 实测值） */
const searchPool = [
    { id: "krypton", name: "Krypton", description: "轻量级协议层优化，显著降低服务端网络开销", author: "modmuss50", downloads: 12_040_000, clientSide: "unsupported", serverSide: "required" },
    { id: "c2me", name: "C2ME", description: "并发化区块生成与写入，提升跑图性能", author: "ishland", downloads: 3_820_000, clientSide: "optional", serverSide: "required" },
    { id: "ferritecore", name: "FerriteCore", description: "内存占用优化，适合大型整合包", author: "malte0612", downloads: 9_150_000, clientSide: "required", serverSide: "required" },
    { id: "spark", name: "spark", description: "服务端性能分析器，火焰图与内存采样", author: "lucko", downloads: 15_600_000, clientSide: "unsupported", serverSide: "required" },
    { id: "ksyxis", name: "Ksyxis", description: "跳过原版世界生成的无效区域加载", author: "Dreeya", downloads: 2_100_000, clientSide: "unsupported", serverSide: "required" },
    { id: "moreculling", name: "More Culling", description: "更激进的实体与方块剔除，提高帧数", author: "fxmorin", downloads: 4_500_000, clientSide: "required", serverSide: "unsupported" },
    { id: "noisemax", name: "Noisemax", description: "生物群系与噪声生成优化", author: "thegggg", downloads: 980_000, clientSide: "required", serverSide: "optional" },
    { id: "voxelmap", name: "VoxelMap", description: "客户端小地图（服务端转换将剔除）", author: "wover", downloads: 6_700_000, clientSide: "required", serverSide: "unsupported" },
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
            clientSide: m.clientSide as SideFlag,
            serverSide: m.serverSide as SideFlag,
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

/** Mod Detail：Krypton 的版本行（整行点击下载；构建级端声明随版本一起返回） */
export const mockModVersions: ModVersionEntry[] = [
    { id: "v-0.2.3", versionNumber: "0.2.3", mcVersion: "1.20.1", loader: "fabric", date: "2023-11-02", sizeBytes: 412_000, recommended: true, url: "https://cdn.modrinth.com/data/fabric-krypton/versions/v-0.2.3/krypton-0.2.3.jar", sha1: "a3d5f1c09b7e2d46c8a05e1f3b7d9c2e4a6c8e01", fileName: "krypton-0.2.3.jar", clientSide: "unsupported", serverSide: "required" },
    { id: "v-0.2.2", versionNumber: "0.2.2", mcVersion: "1.20.1", loader: "fabric", date: "2023-08-19", sizeBytes: 410_500, recommended: false, url: "https://cdn.modrinth.com/data/fabric-krypton/versions/v-0.2.2/krypton-0.2.2.jar", sha1: "b4e6a2d10c8f3e57d9b16f2a4c8e0d3f5b7d9e02", fileName: "krypton-0.2.2.jar", clientSide: "unsupported", serverSide: "required" },
    { id: "v-0.2.1", versionNumber: "0.2.1", mcVersion: "1.20", loader: "fabric", date: "2023-06-07", sizeBytes: 408_100, recommended: false, url: "https://cdn.modrinth.com/data/fabric-krypton/versions/v-0.2.1/krypton-0.2.1.jar", sha1: "c5f7b3e21d9a4f68e0c27a3b5d9f1e4a6c8e0f03", fileName: "krypton-0.2.1.jar", clientSide: "optional", serverSide: "required" },
    { id: "v-0.2.0", versionNumber: "0.2.0", mcVersion: "1.19.4", loader: "fabric", date: "2023-02-14", sizeBytes: 402_900, recommended: false, url: "https://cdn.modrinth.com/data/fabric-krypton/versions/v-0.2.0/krypton-0.2.0.jar", fileName: "krypton-0.2.0.jar", clientSide: "optional", serverSide: "required" },
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
    motd: "A Minecraft server",
    maxPlayers: 20,
    gamemode: "survival",
    difficulty: "easy",
    onlineMode: true,
    levelSeed: "",
    useAikarFlags: false,
    extraJvmArgs: "",
    outputOverride: "",
    keepDirs: [],
    installLoaderLocally: false,
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
