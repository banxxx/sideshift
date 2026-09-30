/**
 * 回收站数据源：本次会话内删掉的任务（Rust 侧只存内存、关应用即清空，故不落地）。
 * 住模块级 store 而非页面 state：垃圾桶挂在应用外壳、删除发生在任务页，两边读同一份；且删除那一帧就要量到落点。
 * `stageDeleted` 只是乐观占位，真正的账每次操作后由 `syncTrash()` 以后端为准覆盖。
 */
import { useSyncExternalStore } from "react";
import * as api from "@/lib/api";
import type { ConversionTask, TrashEntry } from "@/lib/types";

let entries: TrashEntry[] = [];
const listeners = new Set<() => void>();

/** 任务列表重取信号：撤回动作在弹窗里、列表在页面里，靠它把「立刻重取」跨过去 */
let reloadTick = 0;
const reloadListeners = new Set<() => void>();

function emit() {
    listeners.forEach((l) => l());
}

function setEntries(next: TrashEntry[]) {
    entries = next;
    emit();
}

function toEntry(task: ConversionTask, deletedAt = Date.now()): TrashEntry {
    return {
        taskId: task.id,
        packFileName: task.pack.fileName,
        loader: task.pack.loader,
        mcVersion: task.options.mcVersion,
        status: task.status,
        outputFileName: task.outputFileName,
        outputSizeBytes: task.outputSizeBytes,
        deletedAt,
    };
}

const byRecentDeleted = (a: TrashEntry, b: TrashEntry) => b.deletedAt - a.deletedAt;

/** 删除瞬间乐观占位（飞行落点靠它）；`commitDelete` 之后以后端为准 */
export function stageDeleted(task: ConversionTask): void {
    if (entries.some((e) => e.taskId === task.id)) return;
    setEntries([toEntry(task), ...entries].sort(byRecentDeleted));
}

/** 追回/删除失败时撤销占位，别让垃圾桶亮着一条其实没删的记录 */
export function unstageDeleted(id: string): void {
    if (!entries.some((e) => e.taskId === id)) return;
    setEntries(entries.filter((e) => e.taskId !== id));
}

/** 与后端对账：页面挂载时一次（webview 刷新但进程还活着时，回收站是真的还在） */
export async function syncTrash(): Promise<void> {
    try {
        setEntries((await api.listTrash()).sort(byRecentDeleted));
    } catch {
        // 拉不到就维持现状：垃圾桶宁可多亮一条，也不要在删除后凭空消失
    }
}

/** 提交删除（Rust 侧搬进回收站、暂存目录保留）。失败照旧抛出，由调用方提示 */
export async function commitDelete(id: string): Promise<void> {
    await api.deleteTask(id);
    await syncTrash();
}

/** 撤回删除：任务回到列表，同时叫任务页立刻重取（不等那 1s 轮询） */
export async function restoreDeleted(id: string): Promise<void> {
    await api.restoreTask(id);
    await syncTrash();
    reloadTick++;
    reloadListeners.forEach((l) => l());
}

/** 清空回收站，返回丢弃条数（这一步 Rust 才真正删暂存目录） */
export async function clearDeleted(): Promise<number> {
    const n = await api.clearTrash();
    await syncTrash();
    return n;
}

function subscribe(cb: () => void): () => void {
    listeners.add(cb);
    return () => void listeners.delete(cb);
}

export function useTrash(): TrashEntry[] {
    return useSyncExternalStore(subscribe, () => entries);
}

/** 任务页订阅它，变化时立刻重取列表 */
export function useTasksReloadTick(): number {
    return useSyncExternalStore((cb) => {
        reloadListeners.add(cb);
        return () => void reloadListeners.delete(cb);
    }, () => reloadTick);
}
