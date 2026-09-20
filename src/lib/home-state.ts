/**
 * Home 页状态机 hooks（对应 SS.pen Home 三态：Idle / Parsing·Ready / 转换中实况）
 *
 * - useTauriFileDrop：Tauri 下 OS 文件拖入走 webview 的 onDragDropEvent
 *   （HTML5 drop 拿不到真实路径），这里统一订阅并把 {paths} 合成事件交给页面；
 *   浏览器 dev 下该订阅是 no-op，由 Dropzone 自带兜底。
 *   选包/解析状态本身已提升到 App 级 @/lib/pack-store（切标签不丢）。
 * - useActiveTask：当前活跃/最近完成任务。事件驱动（onProgress）+ 1s 轮询兜底
 *   （mock 引擎与完成态切换都靠它；无后端时全部走 mock 数据）。
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import * as api from "@/lib/api";
import { isTauri } from "@/lib/api";
import type { ConversionTask } from "@/lib/types";

/**
 * Tauri webview 文件拖入 → 回调路径列表；浏览器环境下不注册。
 * 返回"OS 文件正拖拽悬停于窗口"标志：Tauri 下 DOM drag 事件收不到文件，
 * 拖拽高亮只能靠这里的 enter/over/leave 事件驱动（交给 Dropzone 展示）。
 */
export function useTauriFileDrop(onPaths: (paths: string[]) => void): boolean {
    const cb = useRef(onPaths);
    cb.current = onPaths;
    const [dragging, setDragging] = useState(false);
    useEffect(() => {
        if (!isTauri) return;
        let unlisten: (() => void) | undefined;
        getCurrentWebview()
            .onDragDropEvent((event) => {
                const type = event.payload.type;
                if (type === "drop") {
                    setDragging(false);
                    cb.current(event.payload.paths);
                } else if (type === "enter" || type === "over") {
                    setDragging(true);
                } else if (type === "leave") {
                    setDragging(false);
                }
            })
            .then((fn) => {
                unlisten = fn;
            });
        return () => unlisten?.();
    }, []);
    return dragging;
}

const ACTIVE: ConversionTask["status"][] = ["queued", "running"];

/**
 * 活跃任务（当前实况小窗数据源）：queued/running 中 createdAt 最新的一个；
 * 无活跃任务时返回最近一次结束的任务（成功/失败/取消），用于全绿终态展示。
 */
export function useActiveTask() {
    const [active, setActive] = useState<ConversionTask | null>(null);
    const alive = useRef(true);
    // 进度事件合并窗：一次转换可以打出成百上千条日志，逐事件全量拉任务列表
    // （含日志数组）会直接把 webview 打满——实测 7000+ 事件时整个应用卡死
    const pendingEvent = useRef(false);

    const refresh = useCallback(async () => {
        // Phase 2 之前 Rust 侧还没有 list_tasks，Tauri 下会 reject；
        // 这里吞掉异常保持空态，避免 1s 轮询把控制台刷满未捕获拒绝。
        let list: ConversionTask[];
        try {
            list = await api.listTasks();
        } catch {
            return;
        }
        if (!alive.current) return;
        const sorted = [...list].sort((a, b) => b.createdAt - a.createdAt);
        const running = sorted.find((t) => ACTIVE.includes(t.status));
        setActive(running ?? sorted.find((t) => t.status !== "queued") ?? null);
    }, []);

    const scheduleRefresh = useCallback(() => {
        if (pendingEvent.current) return;
        pendingEvent.current = true;
        window.setTimeout(() => {
            pendingEvent.current = false;
            void refresh();
        }, 250);
    }, [refresh]);

    useEffect(() => {
        alive.current = true;
        refresh();
        // 进度事件到达时合并拉一次最新快照（事件载荷只带增量，任务整体以 listTasks 为准）
        let unlisten: (() => void) | undefined;
        api.onProgress(scheduleRefresh).then((fn) => {
            unlisten = fn;
        });
        // 轮询兜底：mock 引擎推进、任务完成态切换
        const timer = window.setInterval(refresh, 1000);
        return () => {
            alive.current = false;
            unlisten?.();
            window.clearInterval(timer);
        };
    }, [refresh, scheduleRefresh]);

    return { active, refresh };
}
