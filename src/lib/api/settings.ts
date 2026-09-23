/** 应用设置与更新检查 */
import type {
    AppSettings,
    CacheCleanMode,
    CacheUsage,
    CleanReport,
    VersionOption,
} from "@/lib/types";
import * as mock from "@/lib/mock";
import { invokeOrMock, isTauri } from "./client";

/** 应用版本（设置页「版本」行展示，构建期常量） */
export const APP_VERSION = "0.1.0";

/** 项目仓库地址（设置页 GitHub 按钮） */
export const REPO_URL = "https://github.com/banxxx/sideshift";

/** 读取设置（Rust: get_settings；mock 走 localStorage） */
export async function getSettings(): Promise<AppSettings> {
    if (!isTauri) return mock.mockLoadSettings();
    return invokeOrMock("get_settings", undefined, () => mock.mockLoadSettings());
}

/** 保存设置（Rust: set_settings -> Result；写盘失败会 reject，由设置页外显并回滚） */
export async function saveSettings(s: AppSettings): Promise<void> {
    if (!isTauri) return mock.mockSaveSettings(s);
    return invokeOrMock("set_settings", { settings: s }, () =>
        mock.mockSaveSettings(s)
    );
}

/** 下载源候选（Rust: list_download_sources） */
export async function listDownloadSources(): Promise<VersionOption[]> {
    if (!isTauri) return mock.mockDownloadSources;
    return invokeOrMock("list_download_sources", undefined, () => mock.mockDownloadSources);
}

/** 打开系统目录选择器（设置页两行目录的「选择」，取消返回 null） */
export async function pickDirectory(): Promise<string | null> {
    if (!isTauri) return "D:\\SideShift\\cache";
    const { open } = await import("@tauri-apps/plugin-dialog");
    const picked = await open({ directory: true, multiple: false });
    return typeof picked === "string" ? picked : null;
}

/**
 * 缓存占用（Rust: cache_usage）。一次真实目录扫描，巨包缓存可能几千条目，
 * 所以只在进设置页和清理之后各读一次，不做轮询。
 */
export async function getCacheUsage(): Promise<CacheUsage> {
    if (!isTauri) return mock.mockCacheUsage();
    return invokeOrMock("cache_usage", undefined, () => mock.mockCacheUsage());
}

/**
 * 清理无用文件（Rust: clean_junk）：半截下载 + 孤儿暂存目录 + 空壳目录。
 * 不碰下载缓存本体，所以有任务在跑时也能点。
 */
export async function cleanJunk(): Promise<CleanReport> {
    if (!isTauri) return mock.mockCleanJunk();
    return invokeOrMock("clean_junk", undefined, () => mock.mockCleanJunk());
}

/**
 * 清理下载缓存（Rust: clean_cache）。
 * mode=stale 只删超过 staleDays 未再使用的；mode=all 清空。all 在有任务运行/排队时
 * 由后端 reject（删掉流水线正在取的文件会做出坏包），调用方要把那句话显示出来。
 */
export async function cleanCache(mode: CacheCleanMode): Promise<CleanReport> {
    if (!isTauri) return mock.mockCleanCache(mode);
    return invokeOrMock("clean_cache", { mode }, () => mock.mockCleanCache(mode));
}

/** 检查更新（Rust: check_update -> 最新版本号；与当前版本相同即无更新） */
export async function checkUpdate(): Promise<string> {
    if (!isTauri) return APP_VERSION;
    return invokeOrMock("check_update", undefined, () => APP_VERSION);
}
