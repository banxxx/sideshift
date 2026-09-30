/**
 * 窗口控制共享封装（Tauri 无边框窗口的 minimize/maximize/close），TitleBar、快捷键、页面内按钮共用。
 * 非 Tauri 环境（纯浏览器 dev）必须安全降级为 no-op。
 */
import { useCallback, useEffect, useState } from "react";
import { getCurrentWindow, type Window } from "@tauri-apps/api/window";
import { isTauri } from "@/lib/api";

/** 惰性获取当前窗口；非 Tauri 环境返回 null，调用方据此 no-op */
function getWindow(): Window | null {
    if (!isTauri) return null;
    try {
        return getCurrentWindow();
    } catch {
        return null;
    }
}

export interface WindowControls {
    minimize: () => void;
    toggleMaximize: () => void;
    close: () => void;
    /** 当前是否最大化（浏览器恒为 false） */
    isMaximized: boolean;
}

/** 响应式窗口控制 hook：内部订阅 resize 以同步最大化状态 */
export function useWindowControls(): WindowControls {
    const [maximized, setMaximized] = useState(false);

    useEffect(() => {
        const win = getWindow();
        if (!win) return;
        let unlisten: (() => void) | undefined;
        win.isMaximized().then(setMaximized).catch(() => {});
        win
            .onResized(() => win.isMaximized().then(setMaximized).catch(() => {}))
            .then((fn) => {
                unlisten = fn;
            });
        return () => unlisten?.();
    }, []);

    // 处理器用 useCallback 固定引用，避免每次渲染生成新函数
    const minimize = useCallback(() => getWindow()?.minimize(), []);
    const toggleMaximize = useCallback(() => getWindow()?.toggleMaximize(), []);
    const close = useCallback(() => getWindow()?.close(), []);

    return { minimize, toggleMaximize, close, isMaximized: maximized };
}
