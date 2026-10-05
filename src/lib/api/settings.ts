/** 应用设置读写与缓存维护 */
import type {
    AppSettings,
    CacheCleanMode,
    CacheUsage,
    CleanReport,
    VersionOption,
} from "@/lib/types";
import * as mock from "@/lib/mock";
import { invokeOrMock, isTauri } from "./client";

/**
 * 最近一份设置（模块级，跨页面挂载存续）：换页会重挂载设置页，没有它就要先画一帧骨架、
 * 再在淡入途中整页换成真内容（`SettingsPage` 的 `if (!settings) return <SettingsSkeleton />`）。
 * `saveSettings` 成功也写这里——不然用户刚改过的那几档，下一次进页会先闪回旧值。
 */
let lastSettings: AppSettings | null = null;

/** 上一份读到/存掉的设置；null = 这个进程还没碰过设置（首帧仍走骨架） */
export function peekSettings(): AppSettings | null {
    return lastSettings;
}

/** 读取设置（Rust: get_settings；mock 走 localStorage） */
export async function getSettings(): Promise<AppSettings> {
    const s = isTauri
        ? await invokeOrMock<AppSettings>("get_settings", undefined, () => mock.mockLoadSettings())
        : await mock.mockLoadSettings();
    lastSettings = s;
    return s;
}

/** 保存设置（Rust: set_settings -> Result；写盘失败会 reject，由设置页外显并回滚） */
export async function saveSettings(s: AppSettings): Promise<void> {
    if (!isTauri) {
        await mock.mockSaveSettings(s);
        lastSettings = s;
        return;
    }
    await invokeOrMock<void>("set_settings", { settings: s }, () =>
        mock.mockSaveSettings(s)
    );
    // 写在 await 之后：后端拒绝时这份必须还是磁盘上那一份（页面紧接着会重读一次）
    lastSettings = s;
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
 * 所以不做轮询；调用方（设置页）只在目录换了、手动刷新、清理之后才再读一次，
 * 换页重挂载吃它自己留着的那份结果。
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

/**
 * 清理 Loader 复用安装（Rust: clean_installs）：`cache/installs/` 下每个版本一项 100–160 MB。
 * 与下载缓存分档的原因见 Rust 侧：它是复用资产不是垃圾，删了下次转换同版本要整包重装。
 * 有任务运行/排队时后端 reject（安装中的目录正在被取用），调用方要把那句话显示出来。
 */
export async function cleanInstalls(): Promise<CleanReport> {
    if (!isTauri) return mock.mockCleanInstalls();
    return invokeOrMock("clean_installs", undefined, () => mock.mockCleanInstalls());
}
