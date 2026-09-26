/** 设置（浏览器 dev 兜底）：localStorage 持久化，真实实现走 Rust */
import type {
    AppSettings,
    CacheCleanMode,
    CacheUsage,
    CleanReport,
    UpdateChannel,
    UpdateInfo,
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
    autoClassifyOnline: true,
    verifyAfterBuild: false,
    downloadSource: "official",
    concurrency: 6,
    // null = 跟随这一枚包的版本号，与 Rust 的默认一致（dev 下版本串由 api 层带上）
    updateChannel: null,
    // dev 里默认没配 Key：这样「切到 CurseForge 给出去申请的出口」这条路径在浏览器里也能演
    curseforgeApiKey: null,
    // 本机装 Loader 默认开（与 Rust 一致）：开着才出 JDK 判定那一行
    installLoaderLocally: true,
    reuseLoaderInstalls: true,
    // 语言默认跟随系统（与 Rust 的 AppLocale::Auto 一致）：dev 里 navigator 是什么就出什么
    locale: "auto",
};

/**
 * 检查更新（浏览器 dev）：造一条"比本地新"的 release，让「发现新版本」这条界面路径在没后端时也能演。
 * 返回哪一档跟着渠道走，所以设置页里切渠道能立刻看出区别；真实实现比的是 GitHub releases 列表。
 */
export function mockCheckUpdate(current: string, channel: UpdateChannel | null): UpdateInfo {
    const beta = channel === "beta" || (channel === null && current.includes("-"));
    return { current, latest: beta ? "1.0.0-beta.2" : "1.0.1", hasUpdate: true };
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
