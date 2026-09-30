/**
 * Home 页状态 hooks：useTauriFileDrop（Tauri 下 OS 文件拖入走 webview 的 onDragDropEvent，HTML5 drop 拿不到真实路径；浏览器 dev 为 no-op）；
 * useActiveTask（事件驱动 + 1s 轮询兜底）。选包/解析状态已提升到 App 级 @/lib/pack-store（切标签不丢）。
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
 * 从一份任务快照里挑首页小窗该反映的那条（无活跃任务时取最近一次结束的）。
 * 单列出来是给 `useActiveTask` 的首帧初值复用：那条路径读的是 `api.peekTasks()`，
 * 判据必须与 refresh 完全一致，否则换页首帧会先摆错一张卡再被拽过去。
 */
function pickActive(list: ConversionTask[]) {
    const sorted = [...list].sort((a, b) => b.createdAt - a.createdAt);
    return sorted.find((t) => ACTIVE.includes(t.status)) ?? sorted.find((t) => t.status !== "queued") ?? null;
}

/**
 * 活跃任务（当前实况小窗数据源）：queued/running 中 createdAt 最新的一个；
 * 无活跃任务时返回最近一次结束的任务（成功/失败/取消），用于全绿终态展示。
 *
 * `ready` = 第一次快照已经落地（读失败也算落地），或本进程早先读过、缓存还在（见 `api.peekTasks`）。
 * `active === null` 单独看有两种意思：
 * 「确实没有任务」和「还没查过」，首页只有前者该摆拖放大卡；否则冷启动有在跑的任务时，
 * 会先画一帧大拖放卡、再被共享元素动效拽成紧凑卡，看着像界面自己跳了一下。
 */
export function useActiveTask() {
    const [active, setActive] = useState<ConversionTask | null>(() => {
        const cached = api.peekTasks();
        return cached ? pickActive(cached) : null;
    });
    const [ready, setReady] = useState(() => api.peekTasks() !== null);
    const alive = useRef(true);
    // 进度事件合并窗：一次转换可以打出成百上千条日志，逐事件全量拉任务列表
    // （含日志数组）会直接把 webview 打满——实测 7000+ 事件时整个应用卡死
    const pendingEvent = useRef(false);

    const refresh = useCallback(async () => {
        // Phase 2 之前 Rust 侧还没有 list_tasks，Tauri 下会 reject；
        // 这里吞掉异常保持空态，避免 1s 轮询把控制台刷满未捕获拒绝。
        let list: ConversionTask[] | null = null;
        try {
            list = await api.listTasks();
        } catch {
            /* 保持空态 */
        }
        if (!alive.current) return;
        setReady(true);
        if (!list) return;
        setActive(pickActive(list));
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

    return { active, ready, refresh };
}
