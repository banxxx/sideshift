/**
 * IPC 门面：前端唯一允许调用后端的地方。
 *
 * 设计意图（对齐"公共抽取"要求）：
 *  - 页面组件只 import 本模块的函数，绝不直接 invoke/listen —— 后端命令名、事件名、
 *    参数结构全部封装在此，Phase 2 Rust 落地时只改本文件，页面零改动。
 *  - 运行环境自动探测：在 Tauri 内走真实 #[tauri::command]；纯浏览器 dev 下回落到
 *    mock.ts，让 UI 可脱离后端独立开发与演示。
 *  - DEV 下的 Tauri 壳内，若命令尚未落地（Phase 2 前 invoke 报 command not found），
 *    同样回落 mock，保证 `pnpm tauri dev` 也能走通全部 UI 流程；PROD 不受影响。
 *  - 每个真实分支的 invoke 字符串（如 "parse_pack"）即 Phase 2 的 Rust 命令契约清单。
 */
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
    AppSettings,
    ConversionOptions,
    ConversionReport,
    ConversionTask,
    ModSearchPage,
    ModSearchQuery,
    ModVersionEntry,
    PackDirNode,
    PackManifest,
    PlanMod,
    ProgressEvent,
    VersionOption,
} from "./types";
import { EVENTS } from "./types";
import * as mock from "./mock";

/** 是否运行在 Tauri 桌面壳内 */
export const isTauri =
    typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

/**
 * Tauri 分支统一入口：命令存在则走真实后端；
 * DEV 下命令尚未实现（Phase 2 前）时回落 mock，避免壳内预览时静默失败。
 */
async function invokeOrMock<T>(
    command: string,
    args: Record<string, unknown> | undefined,
    fallback: () => T | Promise<T>
): Promise<T> {
    try {
        return await invoke<T>(command, args);
    } catch (e) {
        const msg = e instanceof Error ? e.message : String(e);
        if (import.meta.env.DEV && /not found/i.test(msg)) return fallback();
        throw e;
    }
}

/* ---------------- 解析 / 选项 ---------------- */

/** 打开系统文件选择器，返回选中的 .mrpack/.zip 路径（取消为 null）
 * 注：刻意用 plugin-dialog 模态框——曾试过无 owner 的 rfd 命令以支持"对话框开着
 * 也能拖文件进窗口"，但会引发弹窗失焦系列问题，已回退，勿再改回 */
export async function pickPackFile(): Promise<string | null> {
    if (!isTauri) return mock.mockManifest.fileName;
    const { open } = await import("@tauri-apps/plugin-dialog");
    const picked = await open({
        multiple: false,
        filters: [{ name: "整合包", extensions: ["mrpack", "zip"] }],
    });
    return typeof picked === "string" ? picked : null;
}

/** 打开系统文件选择器选择 .jar（Convert「从本地添加」；浏览器 dev 下返回 mock 文件名） */
export async function pickJarFile(): Promise<string | null> {
    if (!isTauri) return "krypton-0.2.3.jar";
    const { open } = await import("@tauri-apps/plugin-dialog");
    const picked = await open({
        multiple: false,
        filters: [{ name: "模组文件", extensions: ["jar"] }],
    });
    return typeof picked === "string" ? picked : null;
}

/** 解析整合包（Rust: parse_pack(path) -> PackManifest） */
export async function parsePack(path: string): Promise<PackManifest> {
    if (!isTauri) return mock.mockParsePack(path);
    return invokeOrMock("parse_pack", { path }, () => mock.mockParsePack(path));
}

/** MC 版本列表（Rust: list_mc_versions） */
export async function listMcVersions(): Promise<VersionOption[]> {
    if (!isTauri) return mock.mockMcVersions;
    return invokeOrMock("list_mc_versions", undefined, () => mock.mockMcVersions);
}

/** 指定 MC 版本的加载器版本列表（Rust: list_loader_versions） */
export async function listLoaderVersions(
    mcVersion: string
): Promise<VersionOption[]> {
    if (!isTauri) return mock.mockLoaderVersions;
    return invokeOrMock(
        "list_loader_versions",
        { mcVersion },
        () => mock.mockLoaderVersions
    );
}

/** 可用 Java 版本列表（Rust: list_java_versions） */
export async function listJavaVersions(): Promise<VersionOption[]> {
    if (!isTauri) return mock.mockJavaVersions;
    return invokeOrMock("list_java_versions", undefined, () => mock.mockJavaVersions);
}

/** 默认转换选项（Rust: default_options(manifest)） */
export async function defaultOptions(
    manifest: PackManifest
): Promise<ConversionOptions> {
    if (!isTauri) return mock.mockDefaultOptions;
    return invokeOrMock(
        "default_options",
        { manifest },
        () => mock.mockDefaultOptions
    );
}

/** 包内可保留目录树（Rust: list_pack_dirs） */
export async function listPackDirs(): Promise<PackDirNode[]> {
    if (!isTauri) return mock.mockPackDirs;
    return invokeOrMock("list_pack_dirs", undefined, () => mock.mockPackDirs);
}

/* ---------------- 转换方案 ---------------- */

/** 模组处置方案（Rust: get_plan(taskId?) -> PlanMod[]） */
export async function getPlan(): Promise<PlanMod[]> {
    if (!isTauri) return mock.mockPlanMods;
    return invokeOrMock("get_plan", undefined, () => mock.mockPlanMods);
}

/** 剔除清单（Rust: list_excluded_mods） */
export async function listExcludedMods(): Promise<PlanMod[]> {
    if (!isTauri) return mock.mockExcludedMods;
    return invokeOrMock("list_excluded_mods", undefined, () => mock.mockExcludedMods);
}

/* ---------------- 在线添加 ---------------- */

/** 搜索模组（Rust: search_mods(query) -> ModSearchPage） */
export async function searchMods(query: ModSearchQuery): Promise<ModSearchPage> {
    if (!isTauri) return mock.mockSearch(query);
    return invokeOrMock("search_mods", { query }, () => mock.mockSearch(query));
}

/** 某模组的可用构建版本（Rust: list_mod_versions(modId, mcVersion)） */
export async function listModVersions(
    modId: string
): Promise<ModVersionEntry[]> {
    if (!isTauri) return mock.mockModVersions;
    return invokeOrMock("list_mod_versions", { modId }, () => mock.mockModVersions);
}

/* ---------------- 任务生命周期 ---------------- */

/** 创建并开始转换（Rust: start_conversion(options, manifest, plan) -> taskId）；plan 为前端确认过的最终方案 */
export async function startConversion(
    options: ConversionOptions,
    pack: PackManifest,
    plan: PlanMod[]
): Promise<string> {
    if (!isTauri) return mock.mockStartTask(options, pack);
    return invokeOrMock(
        "start_conversion",
        { options, manifest: pack, plan },
        () => mock.mockStartTask(options, pack)
    );
}

/** 任务列表（Rust: list_tasks） */
export async function listTasks(): Promise<ConversionTask[]> {
    if (!isTauri) return mock.mockListTasks();
    return invokeOrMock("list_tasks", undefined, () => mock.mockListTasks());
}

/** 单任务（Rust: get_task(id)） */
export async function getTask(id: string): Promise<ConversionTask | undefined> {
    if (!isTauri) return mock.mockGetTask(id);
    return invokeOrMock("get_task", { id }, () =>
        mock.mockGetTask(id)
    ).then((t) => t ?? undefined);
}

/** 取消任务（Rust: cancel_task(id)） */
export async function cancelTask(id: string): Promise<void> {
    if (!isTauri) return mock.mockCancelTask(id);
    return invokeOrMock("cancel_task", { id }, () => mock.mockCancelTask(id));
}

/** 重试任务（Rust: retry_task(id) -> newTaskId） */
export async function retryTask(id: string): Promise<string | undefined> {
    if (!isTauri) return mock.mockRetryTask(id);
    return invokeOrMock("retry_task", { id }, () => mock.mockRetryTask(id));
}

/** 删除任务记录（Rust: delete_task(id)） */
export async function deleteTask(id: string): Promise<void> {
    if (!isTauri) return mock.mockDeleteTask(id);
    return invokeOrMock("delete_task", { id }, () => mock.mockDeleteTask(id));
}

/** 转换报告（Rust: get_report(taskId)） */
export async function getReport(taskId: string): Promise<ConversionReport | undefined> {
    if (!isTauri) return mock.mockReport(taskId);
    return invokeOrMock("get_report", { taskId }, () =>
        mock.mockReport(taskId)
    ).then((r) => r ?? undefined);
}

/** 订阅流水线进度事件（浏览器 mock 模式无事件流，返回空取消函数） */
export function onProgress(cb: (e: ProgressEvent) => void): Promise<UnlistenFn> {
    if (!isTauri) return Promise.resolve(() => {});
    return listen<ProgressEvent>(EVENTS.progress, (ev) => cb(ev.payload));
}

/* ---------------- 设置 ---------------- */

/** 应用版本（设置页「版本」行展示，构建期常量） */
export const APP_VERSION = "0.1.0";

/** 项目仓库地址（设置页 GitHub 按钮） */
export const REPO_URL = "https://github.com/banxxx/sideshift";

/** 读取设置（Rust: get_settings；mock 走 localStorage） */
export async function getSettings(): Promise<AppSettings> {
    if (!isTauri) return mock.mockLoadSettings();
    return invokeOrMock("get_settings", undefined, () => mock.mockLoadSettings());
}

/** 保存设置（Rust: set_settings） */
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

/** 打开系统目录选择器（设置页「工作缓存目录 · 选择」） */
export async function pickDirectory(): Promise<string | null> {
    if (!isTauri) return "D:\\SideShift\\cache";
    const { open } = await import("@tauri-apps/plugin-dialog");
    const picked = await open({ directory: true, multiple: false });
    return typeof picked === "string" ? picked : null;
}

/** 检查更新（Rust: check_update -> 最新版本号；与当前版本相同即无更新） */
export async function checkUpdate(): Promise<string> {
    if (!isTauri) return APP_VERSION;
    return invokeOrMock("check_update", undefined, () => APP_VERSION);
}

/* ---------------- 系统集成 ---------------- */

/** 按目录串的分隔符风格拼接路径（仅用于展示与定位，不做规范化） */
export function joinPath(dir: string, fileName: string): string {
    const trimmed = dir.replace(/[\\/]+$/, "");
    return `${trimmed}${trimmed.includes("\\") ? "\\" : "/"}${fileName}`;
}

/** 目录串去掉末尾文件名，得到所在目录（报告页「打开文件夹」） */
export function dirOf(path: string): string {
    return path.replace(/[\\/][^\\/]*$/, "");
}

/** 输出文件完整路径 = 输出目录（任务覆写优先，否则全局设置）+ 文件名 */
export async function resolveOutputPath(
    fileName: string,
    overrideDir?: string
): Promise<string> {
    const { outputDir } = await getSettings();
    return joinPath(overrideDir?.trim() || outputDir, fileName);
}

/** 在系统文件管理器中定位文件（浏览器 dev 下为空操作） */
export async function revealPath(path: string): Promise<void> {
    if (!isTauri) return;
    const { revealItemInDir } = await import("@tauri-apps/plugin-opener");
    await revealItemInDir(path);
}

/** 用系统默认程序打开目录（报告页「打开文件夹」/ 列表卡「打开输出目录」） */
export async function openDir(path: string): Promise<void> {
    if (!isTauri) return;
    const { openPath } = await import("@tauri-apps/plugin-opener");
    await openPath(path);
}

/** 在系统浏览器打开外链（设置页 GitHub 按钮） */
export async function openExternal(url: string): Promise<void> {
    if (!isTauri) {
        window.open(url, "_blank", "noopener");
        return;
    }
    const { openUrl } = await import("@tauri-apps/plugin-opener");
    await openUrl(url);
}
