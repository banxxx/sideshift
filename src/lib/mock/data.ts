/**
 * Mock 静态夹具：后端 command 就绪前驱动全部 UI（数据与设计稿 SS.pen 逐屏对齐）。
 * 只放无状态数据与纯函数；流水线定时器在 tasks.ts，分类兜底在 classify.ts。
 */
import type {
    AckList,
    ConversionOptions,
    JavaInstall,
    JavaProbe,
    ModSearchPage,
    ModSearchQuery,
    ModSearchResult,
    ModSourceKind,
    ModVersionEntry,
    PackDirNode,
    PackDirTree,
    PackFileNode,
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

/**
 * Fabric loader 版本下拉。2026-09-28 实测：`meta.fabricmc.net/v2/versions/loader/{mc}` 对
 * 1.16.5 / 1.17.1 / 1.20.1 / 1.20.6 / 1.21.1 返回的是**同一份全量表**（各 253 条、集合逐条相同），
 * 且整表只有最新那一枚带 `stable: true`。所以 mock 也按「一份全表」给，别演成「每档 MC 各有各的号」——
 * 那是 Forge/NeoForge 的形状，不是 Fabric 的。
 */
export const mockLoaderVersions = (_mcVersion: string): VersionOption[] => [
    { value: "0.19.5", label: "0.19.5", recommended: true },
    { value: "0.19.4", label: "0.19.4" },
    { value: "0.16.9", label: "0.16.9" },
    { value: "0.15.3", label: "0.15.3" },
    { value: "0.14.24", label: "0.14.24" },
];

/**
 * 这台"假机器"上躺着的三枚 JDK：两枚同版本的 21 是给下拉的 `(1)/(2)` 编号看的，
 * 一枚 17 用来演「手选了一枚低于需求线」那一档的红字与拦截。顺序即候选顺序（JAVA_HOME → PATH）。
 */
const mockInstalls: JavaInstall[] = [
    { major: 21, path: "C:\\Program Files\\Java\\jdk-21\\bin\\java.exe" },
    { major: 21, path: "C:\\Users\\Ban\\scoop\\apps\\openjdk21\\current\\bin\\java.exe" },
    { major: 17, path: "C:\\Program Files\\Eclipse Adoptium\\jdk-17\\bin\\java.exe" },
];

/** 本机 JDK 探测（浏览器 dev）：判据与文案都照 `core::java::probe` 那份写，别让两套环境各说一套 */
export function mockJavaProbe(requiredVersion: string | null, javaPath: string | null): JavaProbe {
    const required = requiredVersion
        ? Number.parseInt(requiredVersion.replace(/\D/g, ""), 10)
        : Number.NaN;
    const need = Number.isNaN(required) ? null : required;
    const wanted = javaPath?.trim() || null;
    const hit = wanted ? mockInstalls.find((j) => j.path === wanted) : undefined;
    const chosen =
        hit ?? mockInstalls.find((j) => need === null || j.major >= need) ?? mockInstalls[0];
    const enough = need === null || chosen.major >= need;
    return {
        status: enough ? "pass" : "fail",
        javaPath: chosen.path,
        major: chosen.major,
        requiredMajor: need,
        installed: mockInstalls,
        selectedMissing: !!wanted && !hit,
        detail: enough
            ? `已检测到 Java ${chosen.major}${need ? `（本次需要 Java ${need} 及以上）` : ""}`
            : `本机 Java ${chosen.major} 低于本次需要的 Java ${need}，转换会在这里失败`,
    };
}

/**
 * MC 版本号 → 「版本线 + 补丁号」，与 Rust 的 `core::mc_version::parse` 逐条同口径：
 * 老编号 `1.x.y` 的线在第二段，26 起的年份线就是首段（`26.3` → 26），老 Beta/Alpha 的字母前缀先剥。
 * 整段严格转整数（`6-fabric` 不读成 6、`a0` 读不出就整串认不出），别用 `parseInt` 的前缀读法。
 */
function mockMcLine(mc: string): [number, number] | null {
    const seg = mc.trim().replace(/^[A-Za-z]+/, "").split(".");
    const num = (s: string | undefined) => (/^\d+$/.test(s ?? "") ? Number(s) : null);
    const first = num(seg[0]);
    if (first === null) return null;
    if (first === 1) {
        const line = num(seg[1]);
        return line === null ? null : [line, num(seg[2]) ?? 0];
    }
    return [first, num(seg[1]) ?? 0];
}

/**
 * MC 版本 → Java 需求线（浏览器 dev）。表抄 `core::java::required_for_mc`，档位按 piston-meta
 * 官方 `javaVersion.majorVersion` 实测：8 = …1.16.5、16 = 1.17、17 = 1.18–1.20.4、
 * 21 = 1.20.5–1.21.11、**25 = 26.x**；认不出形状的按 1.20 兜底 = 17。两边改了要记得同步。
 * 真机上这条命令还有第一腿（直接问官方字段，命中本地表零请求），mock 没有网络 ⇒ 只演那张兜底表。
 */
export function mockJavaForMc(mc: string): string {
    const [line, patch] = mockMcLine(mc) ?? [20, 0];
    if (line >= 26) return "25";
    if (line > 20 || (line === 20 && patch >= 5)) return "21";
    if (line >= 18) return "17";
    if (line === 17) return "16";
    return "8";
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
            // Modrinth 的 id 本来就是 slug（CurseForge 才另给一栏），「翻译」按钮认的是这个
            slug: m.id,
            clientSide: m.clientSide as SideFlag,
            serverSide: m.serverSide as SideFlag,
            iconUrl: undefined,
            source: query.source,
            compatible: true,
            // 「已在方案里」按 id 对到 mock 方案（后端不再自动塞推荐模组，写死某一条就会和方案样本脱钩）
            alreadyAdded: mockPlanMods.some((p) => p.id === m.id),
        })),
    };
}

/** 单项目展示信息（前置直跳详情用）：mock 池里认得就给真的，认不得就按 id 拼一个 */
export function mockModDetail(source: ModSourceKind, modId: string): ModSearchResult {
    const hit = searchPool.find((m) => m.id === modId);
    if (hit) {
        return {
            ...hit,
            slug: hit.id,
            clientSide: hit.clientSide as SideFlag,
            serverSide: hit.serverSide as SideFlag,
            iconUrl: undefined,
            source,
            compatible: true,
            alreadyAdded: false,
        };
    }
    return {
        id: modId,
        name: `Mock Mod ${modId}`,
        description: "（mock）这是一个按 id 拼出来的模拟详情。",
        author: "mock-author",
        downloads: 12_345,
        source,
        compatible: true,
        alreadyAdded: false,
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
    { id: "v-0.2.3", versionNumber: "0.2.3", mcVersion: "1.20.1", loader: "fabric", date: "2023-11-02", sizeBytes: 412_000, recommended: true, url: "https://cdn.modrinth.com/data/fabric-krypton/versions/v-0.2.3/krypton-0.2.3.jar", sha1: "a3d5f1c09b7e2d46c8a05e1f3b7d9c2e4a6c8e01", fileName: "krypton-0.2.3.jar", clientSide: "unsupported", serverSide: "required", depends: [{ id: "P7dR8mSH", name: "Fabric API", slug: "fabric-api", required: true }, { id: "mQtYetb2", name: "Cloth Config API", slug: "cloth-config", required: false }] },
    { id: "v-0.2.2", versionNumber: "0.2.2", mcVersion: "1.20.1", loader: "fabric", date: "2023-08-19", sizeBytes: 410_500, recommended: false, url: "https://cdn.modrinth.com/data/fabric-krypton/versions/v-0.2.2/krypton-0.2.2.jar", sha1: "b4e6a2d10c8f3e57d9b16f2a4c8e0d3f5b7d9e02", fileName: "krypton-0.2.2.jar", clientSide: "unsupported", serverSide: "required" },
    { id: "v-0.2.1", versionNumber: "0.2.1", mcVersion: "1.20", loader: "fabric", date: "2023-06-07", sizeBytes: 408_100, recommended: false, url: "https://cdn.modrinth.com/data/fabric-krypton/versions/v-0.2.1/krypton-0.2.1.jar", sha1: "c5f7b3e21d9a4f68e0c27a3b5d9f1e4a6c8e0f03", fileName: "krypton-0.2.1.jar", clientSide: "optional", serverSide: "required" },
    { id: "v-0.2.0", versionNumber: "0.2.0", mcVersion: "1.19.4", loader: "fabric", date: "2023-02-14", sizeBytes: 402_900, recommended: false, url: "https://cdn.modrinth.com/data/fabric-krypton/versions/v-0.2.0/krypton-0.2.0.jar", fileName: "krypton-0.2.0.jar", clientSide: "optional", serverSide: "required" },
];

export const mockDefaultOptions: ConversionOptions = {
    mcVersion: "1.20.1",
    loaderVersion: "0.15.3",
    javaVersion: "21",
    javaPath: "",
    memoryMb: 6144,
    generateScripts: true,
    nogui: false,
    // 两档默认开，与 Rust 的默认一致：eula 关着做出来的包首次拒启；Aikar 是官方推荐参数组
    agreeEula: true,
    serverPort: 25565,
    motd: "A Minecraft server",
    maxPlayers: 20,
    gamemode: "survival",
    difficulty: "easy",
    onlineMode: true,
    levelSeed: "",
    useAikarFlags: true,
    extraJvmArgs: "",
    outputOverride: "",
    keepDirs: [],
    keepFiles: [],
    installLoaderLocally: true,
};

/** 演示树里的文件条目：`path` 就是 `keepFiles` 的取值（从包根起算、小写、已剥 overrides 壳） */
const packFile = (path: string, sizeBytes: number): PackFileNode => ({
    name: path.split("/").pop() ?? path,
    sizeBytes,
    path,
});

/** 演示树里的目录节点：fileCount / sizeBytes 由「直属文件 + 子目录」递归聚合，
 *  口径与后端 `list_pack_dirs` 一致——手填对不上的话，弹窗那行读数当场露馅 */
const packDir = (
    name: string,
    files: PackFileNode[] = [],
    children: PackDirNode[] = []
): PackDirNode => ({
    name,
    files,
    children,
    fileCount: files.length + children.reduce((s, d) => s + d.fileCount, 0),
    sizeBytes:
        files.reduce((s, f) => s + f.sizeBytes, 0) + children.reduce((s, d) => s + d.sizeBytes, 0),
});

/** 包内可保留内容树（客户端保留目录弹窗演示数据） */
export const mockPackTree: PackDirTree = {
    dirs: [
        packDir(
            "config",
            [
                packFile("config/sodium-options.json", 3_182),
                packFile("config/fabricloader.properties", 240),
            ],
            [
                packDir("jei", [
                    packFile("config/jei/jei.ini", 2_048),
                    packFile("config/jei/item-blacklist.txt", 96),
                ]),
                packDir("sombreros", [
                    packFile("config/sombreros/sombreros.toml", 1_180),
                    packFile("config/sombreros/player_models.toml", 320),
                ]),
            ]
        ),
        packDir(
            "kubejs",
            [packFile("kubejs/startup.js", 1_420)],
            [
                packDir("client_scripts", [
                    packFile("kubejs/client_scripts/tooltips.js", 2_880),
                    packFile("kubejs/client_scripts/ping.js", 640),
                ]),
                packDir("server_scripts", [
                    packFile("kubejs/server_scripts/recipes.js", 7_120),
                ]),
            ]
        ),
        packDir("shaderpacks", [
            packFile("shaderpacks/complementary-reimagined.zip", 5_242_880),
            packFile("shaderpacks/bsl.zip", 3_774_873),
        ]),
        packDir("maps", [], [packDir("journey_map", [packFile("maps/journey_map/map.dat", 18_208)])]),
    ],
    files: [
        packFile("options.txt", 4_190),
        packFile("optionsof.txt", 1_204),
        packFile("servers.dat", 512),
        // 这两枚的名字 builder 写死了：勾上就走「以包内那份为准」那一档，界面上对应的设置项要当场灰
        packFile("eula.txt", 20),
        packFile("server.properties", 1_024),
    ],
};

/**
 * 鸣谢名单的浏览器 dev 夹具。真名单在远端（Rust: `core::ack`），这份只保证纯浏览器
 * 开发时那一整页有东西可看——包括 3D 皮肤头像那一层：`mockAckSkins` 给的是 Mojang 公开的
 * 贴图地址（真地址、真走一次 CDN，只是「名字→地址」那一段在纯浏览器里打不了，故照抄一份）。
 */
export const mockAckList: AckList = {
    version: "mock",
    people: [
        { name: "Banxxx", minecraftId: false },
        { name: "POSOO", minecraftId: true },
    ],
};

/** 与 `mockAckList` 同一形状的一份对照表：只有 `minecraftId` 为真的名字会出现在这里 */
export const mockAckSkins: Record<string, string> = {
    POSOO: "https://textures.minecraft.net/texture/9b49d068923369682cafc31f50f93cb35c437358cddebc7feff5566fb943e8b5",
};
