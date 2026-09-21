/**
 * IPC 底座：环境探测 + 「命令缺失回落 mock」的统一入口。
 * 除本目录内的域文件外，别处不应直接 invoke/listen。
 */
import { invoke } from "@tauri-apps/api/core";

/** 是否运行在 Tauri 桌面壳内 */
export const isTauri =
    typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

/**
 * Tauri 分支统一入口：命令存在则走真实后端；
 * DEV 下命令尚未实现（Phase 2 前）时回落 mock，避免壳内预览时静默失败。
 */
export async function invokeOrMock<T>(
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
