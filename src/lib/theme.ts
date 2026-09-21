/**
 * 主题存储：light / dark / system 三态，唯一真源。
 *
 * 侧栏底部按钮与设置页「主题」分段都消费这里，避免两处各写一份 localStorage 导致状态不同步。
 * key 沿用 "theme"，与 index.html 首帧内联脚本约定一致（防深色模式闪白）。
 * 未设置过的一律落在 "system"：两处默认值必须同步，否则首帧与挂载后不是同一个主题。
 */
import { useSyncExternalStore } from "react";

export type Theme = "light" | "dark" | "system";

const KEY = "theme";

/** 没存过（或存了个不认识的串）都按系统走，不预设深浅 */
function read(): Theme {
    const v = localStorage.getItem(KEY);
    return v === "light" || v === "dark" || v === "system" ? v : "system";
}

const media = () => window.matchMedia("(prefers-color-scheme: dark)");

let theme: Theme = read();
const listeners = new Set<() => void>();

function emit() {
    document.documentElement.classList.toggle("dark", theme === "system" ? media().matches : theme === "dark");
    listeners.forEach((l) => l());
}

export function setTheme(next: Theme): void {
    theme = next;
    localStorage.setItem(KEY, next);
    emit();
}

/** 当前是否渲染为深色（侧栏按钮图标用） */
export function isDark(): boolean {
    return document.documentElement.classList.contains("dark");
}

export function useTheme(): [Theme, (t: Theme) => void] {
    const value = useSyncExternalStore(
        (cb) => {
            listeners.add(cb);
            return () => void listeners.delete(cb);
        },
        () => theme
    );
    return [value, setTheme];
}

// 模块载入即对齐一次 DOM，并在系统主题变化时刷新 system 态
emit();
media().addEventListener("change", () => {
    if (theme === "system") emit();
});
