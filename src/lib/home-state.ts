/**
 * Home 页状态机 hooks（对应 SS.pen Home 三态：Idle / Parsing·Ready / 转换中实况）
 *
 * - usePackSelection：文件选取 + 解析。Tauri 下 OS 文件拖入走 webview 的
 *   onDragDropEvent（HTML5 drop 拿不到真实路径），这里统一订阅并把 {paths}
 *   合成事件交给页面；浏览器 dev 下该订阅是 no-op，由 Dropzone 自带兜底。
 * - useActiveTask：当前活跃/最近完成任务。事件驱动（onProgress）+ 1s 轮询兜底
 *   （mock 引擎与完成态切换都靠它；无后端时全部走 mock 数据）。
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import * as api from "@/lib/api";
import { isTauri } from "@/lib/api";
import type { ConversionTask, PackManifest } from "@/lib/types";

/** 拖放/选择 → 解析 一体化状态 */
export function usePackSelection() {
    const [manifest, setManifest] = useState<PackManifest | null>(null);
    const [parsing, setParsing] = useState(false);
    const [error, setError] = useState<string | null>(null);
    /** 解析失败的文件名：错误态卡片仍要显示"是哪个包失败了" */
    const [errorName, setErrorName] = useState<string | null>(null);

    const parse = useCallback(async (path: string) => {
        setParsing(true);
        setError(null);
        setErrorName(null);
        const fail = (msg: string) => {
            setManifest(null);
            setError(msg);
            setErrorName(path.split(/[\\/]/).pop() ?? path);
        };
        try {
            const m = await api.parsePack(path);
            if (m.parsed) {
                setManifest(m);
            } else {
                fail(m.error ?? "解析失败");
            }
        } catch (e) {
            fail(e instanceof Error ? e.message : String(e));
        } finally {
            setParsing(false);
        }
    }, []);

    const pickByDialog = useCallback(async () => {
        try {
            const path = await api.pickPackFile();
            if (path) await parse(path);
        } catch (e) {
            setError(e instanceof Error ? e.message : String(e));
        }
    }, [parse]);

    const reset = useCallback(() => {
        setManifest(null);
        setError(null);
        setErrorName(null);
        setParsing(false);
    }, []);

    return { manifest, parsing, error, errorName, parse, pickByDialog, reset };
}

/** Tauri webview 文件拖入 → 回调路径列表；浏览器环境下不注册 */
export function useTauriFileDrop(onPaths: (paths: string[]) => void) {
    const cb = useRef(onPaths);
    cb.current = onPaths;
    useEffect(() => {
        if (!isTauri) return;
        let unlisten: (() => void) | undefined;
        getCurrentWebview()
            .onDragDropEvent((event) => {
                if (event.payload.type === "drop") cb.current(event.payload.paths);
            })
            .then((fn) => {
                unlisten = fn;
            });
        return () => unlisten?.();
    }, []);
}

const ACTIVE: ConversionTask["status"][] = ["queued", "running"];

/**
 * 活跃任务（当前实况小窗数据源）：queued/running 中 createdAt 最新的一个；
 * 无活跃任务时返回最近一次结束的任务（成功/失败/取消），用于全绿终态展示。
 */
export function useActiveTask() {
    const [active, setActive] = useState<ConversionTask | null>(null);
    const alive = useRef(true);

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

    useEffect(() => {
        alive.current = true;
        refresh();
        // 进度事件到达时立刻拉一次最新快照（事件载荷只带增量，任务整体以 listTasks 为准）
        let unlisten: (() => void) | undefined;
        api.onProgress(() => void refresh()).then((fn) => {
            unlisten = fn;
        });
        // 轮询兜底：mock 引擎推进、任务完成态切换
        const timer = window.setInterval(refresh, 1000);
        return () => {
            alive.current = false;
            unlisten?.();
            window.clearInterval(timer);
        };
    }, [refresh]);

    return { active, refresh };
}
