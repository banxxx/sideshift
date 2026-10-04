/** 设置（浏览器 dev 兜底）：localStorage 持久化，真实实现走 Rust */
import type {
    AppSettings,
    CacheCleanMode,
    CacheUsage,
    CleanReport,
    UpdateChannel,
    UpdateInfo,
    UpdateOutcome,
    UpdateStatus,
    VersionOption,
} from "@/lib/types";

const SETTINGS_KEY = "sideshift.settings";

/**
 * 默认目录对与 Rust 侧同构：`{数据根}\{output,cache}`。
 * 真实数据根由 Rust 决定（便携包 → exe 同级 data；安装器指定 → 那个根；否则预选非系统盘），
 * 浏览器 dev 没这些概念，写死这台机器常见的 D: 只为让界面有东西可显示。
 */
export const mockDefaultSettings: AppSettings = {
    outputDir: "D:\\SideShift\\output",
    cacheDir: "D:\\SideShift\\cache",
    stripClientOnly: true,
    // 与 Rust 一致：默认问官方。选 `minekuai` 才能演「先自查存活 → 镜像答上 → 官方腿一条不发」
    envLookupSource: "official",
    // 与 Rust 一致：百科腿默认关（非官方行为 + HTML 解析脆弱，见类型注释）
    envLookupMcmod: false,
    verifyAfterBuild: false,
    downloadSource: "official",
    concurrency: 6,
    // null = 跟随这一枚包的版本号，与 Rust 的默认一致（dev 下版本串由 api 层带上）
    updateChannel: null,
    // 与 Rust 一致：Modrinth 查询优先走 mcimirror、官方兜底
    modrinthMirror: true,
    // 本机装 Loader 默认开（与 Rust 一致）：开着才出 JDK 判定那一行
    installLoaderLocally: true,
    reuseLoaderInstalls: true,
    // 语言默认跟随系统（与 Rust 的 AppLocale::Auto 一致）：dev 里 navigator 是什么就出什么
    locale: "auto",
};

/**
 * 检查更新（浏览器 dev）：造一条"比本地新"的 release，让「发现新版本」这条界面路径在没后端时也能演。
 * 返回哪一档跟着渠道走，所以设置页里切渠道能立刻看出区别；真实实现比的是 GitHub releases 列表。
 *
 * `downloadable` 跟着产物配齐与否走（与 Rust 那张真值表同判据）：取件那一轮已经接上，
 * dev 里就得演得出「点得动」这一态，否则进度与取消那两条路径在浏览器里永远看不见。
 */
export function mockCheckUpdate(current: string, channel: UpdateChannel | null): UpdateInfo {
    const beta = channel === "beta" || (channel === null && current.includes("-"));
    const latest = beta ? "1.0.0-beta.2" : "1.0.1";
    const tag = `v${latest}`;
    return {
        current,
        latest,
        tag,
        hasUpdate: true,
        channel: beta ? "beta" : "stable",
        releaseUrl: `https://github.com/OWNER/REPO/releases/tag/${tag}`,
        publishedAt: "2026-01-01T00:00:00Z",
        notes: "- Mock release notes line one\n- 第二行说明",
        assets: [
            { name: `SideShift_${latest}_x64-setup.exe`, size: 4_636_711, kind: "package", trusted: true },
            { name: `SideShift_${latest}_x64-setup.exe.sig`, size: 428, kind: "signature", trusted: true },
            { name: `SideShift-${latest}-portable-x64.zip`, size: 12_582_912, kind: "portable", trusted: true },
        ],
        downloadable: true,
        blocked: null,
    };
}

/* ================= 取件这一轮的 dev 假象 =================
 * 浏览器里既没后端也没事件总线，所以在 mock 侧自备一只进程内订阅表 + 一条自己走完的假轮次。
 * 档位与 Rust 同序（downloading → verifying → ready），字段照抄 `UpdateStatus`，
 * 免得 dev 演的是另一套状态机。
 */
type StatusListener = (s: UpdateStatus) => void;

const mockIdle: UpdateStatus = { stage: "idle", version: null, downloaded: 0, total: 0, error: null };
let mockStatus: UpdateStatus = mockIdle;
const mockListeners = new Set<StatusListener>();
let mockTimer: ReturnType<typeof setInterval> | undefined;

function emitMock(s: UpdateStatus) {
    mockStatus = s;
    for (const l of mockListeners) l(s);
}

/** 订阅假进度（对应真实侧的 `update://progress`） */
export function onMockUpdateProgress(cb: StatusListener): () => void {
    mockListeners.add(cb);
    return () => void mockListeners.delete(cb);
}

export function mockUpdateStatus(): UpdateStatus {
    return mockStatus;
}

/**
 * 假取件：200ms 一跳走完下载，再停一拍演「正在验签」，最后落 ready。
 * 字节数与 `mockCheckUpdate` 那两个产物对齐（包 + 同名签名），界面里的「x / y MB」才不会自相矛盾
 */
export function mockPrepareUpdate(version: string): UpdateStatus {
    const total = 8_388_608 + 412;
    clearInterval(mockTimer);
    let down = 0;
    emitMock({ stage: "downloading", version, downloaded: 0, total, error: null });
    mockTimer = setInterval(() => {
        down = Math.min(total, down + total / 25);
        emitMock({ stage: "downloading", version, downloaded: down, total, error: null });
        if (down < total) return;
        clearInterval(mockTimer);
        emitMock({ stage: "verifying", version, downloaded: total, total, error: null });
        mockTimer = setTimeout(() => {
            emitMock({ stage: "ready", version, downloaded: total, total, error: null });
        }, 600);
    }, 200);
    return mockStatus;
}

/** 假取消：与 Rust 同口径——正在跑就只立旗（那一轮自己把半截收走），没在跑就当场回 idle */
export function mockCancelUpdate(): UpdateStatus {
    clearInterval(mockTimer);
    emitMock({ ...mockIdle });
    return mockStatus;
}

/**
 * 假安装：浏览器里没有安装器可跑、也不会重启，所以这一句**刻意失败**。
 * 那颗钮要能看出「接线是通的」，而不是点下去什么都不发生；真那条链只在打包后的程序里测。
 * 交回去的是种类码而不是中文整句，与真实侧同一口径（渲染只在 `errors.ts` 一处）
 */
export function mockInstallUpdate(): Promise<void> {
    return Promise.reject("app:update-browser");
}

/** 假结论：浏览器里那次安装没发生 ⇒ 永远没有账可报（与真实侧「从没试过安装」同形） */
export function mockUpdateOutcome(): UpdateOutcome | null {
    return null;
}

/** 下载源下拉（Settings · 网络）：与 Rust `list_download_sources` 同序同文案 */
export const mockDownloadSources: VersionOption[] = [
    { value: "official", label: "官方源", recommended: true },
    { value: "bmclapi", label: "BMCLAPI 国内镜像" },
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

/**
 * 缓存占用（浏览器 dev）：一份可减少的假账，清理动作真的把对应项归零，
 * 免得界面演「已清理 1.2 GB」而数字一动不动。真实实现扫的是磁盘目录。
 */
let mockUsage: CacheUsage = {
    cacheDir: mockDefaultSettings.cacheDir,
    exists: true,
    filesCount: 214,
    filesBytes: 1_258_291_200,
    staleCount: 61,
    staleBytes: 396_365_568,
    partsCount: 3,
    partsBytes: 12_582_912,
    orphanCount: 1,
    orphanBytes: 89_128_960,
    emptyDirs: 7,
    busy: false,
    staleDays: 30,
};

export function mockCacheUsage(): CacheUsage {
    // 目录跟着设置走：dev 里改了缓存目录，界面显示的路径不该还挂着旧值
    return { ...mockUsage, cacheDir: mockLoadSettings().cacheDir };
}

/** 清无用文件：半截下载 + 孤儿暂存 + 空壳目录；下载缓存本体不动 */
export function mockCleanJunk(): CleanReport {
    const r: CleanReport = {
        items: mockUsage.partsCount + mockUsage.orphanCount + mockUsage.emptyDirs,
        bytes: mockUsage.partsBytes + mockUsage.orphanBytes,
        failed: 0,
    };
    mockUsage = {
        ...mockUsage,
        partsCount: 0,
        partsBytes: 0,
        orphanCount: 0,
        orphanBytes: 0,
        emptyDirs: 0,
    };
    return r;
}

/** 清下载缓存：stale 只动过期项，all 连在用的也清掉 */
export function mockCleanCache(mode: CacheCleanMode): CleanReport {
    const all = mode === "all";
    const r: CleanReport = {
        items: all ? mockUsage.filesCount : mockUsage.staleCount,
        bytes: all ? mockUsage.filesBytes : mockUsage.staleBytes,
        failed: 0,
    };
    mockUsage = {
        ...mockUsage,
        filesCount: all ? 0 : mockUsage.filesCount - mockUsage.staleCount,
        filesBytes: all ? 0 : mockUsage.filesBytes - mockUsage.staleBytes,
        staleCount: 0,
        staleBytes: 0,
    };
    return r;
}
