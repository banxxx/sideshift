/**
 * 主题存储：light / dark / system 三态，唯一真源。
 *
 * 侧栏底部按钮与设置页「主题」分段都消费这里，避免两处各写一份 localStorage 导致状态不同步。
 * key 沿用 "theme"，与 index.html 首帧内联脚本约定一致（防深色模式闪白）。
 * 未设置过的一律落在 "system"：两处默认值必须同步，否则首帧与挂载后不是同一个主题。
 *
 * 深浅的判断读**存储态**（`isDark()`），不读 DOM 的 `.dark` 类：类只是存储态的镜像，
 * 而切换动画刻意向后延迟落类（见 `switchTheme`），读类会让触发件上的图标慢半拍。
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

/**
 * 按下到铺开之间的空档：让触发件先把换图 + 光环脉冲演出去。
 * 比脉冲（130ms）略短一点点不成——但也不要长：这段是纯等待，展开本身已经 660ms，
 * 再叠上去点一下要 790ms 才收干净，就从「有编排」读成「卡了一下」。
 * 之所以敢到 660ms：这条曲线的后 1/3 只在墙角爬，几乎不占观感上的演出时间。
 */
const PRE_DELAY = 130;

let theme: Theme = read();
const listeners = new Set<() => void>();
let revealTimer: number | undefined;

/** 存储态 → 该渲染成深还是浅（system 跟随 OS） */
function resolvedDark(): boolean {
    return theme === "system" ? media().matches : theme === "dark";
}

function applyClass() {
    document.documentElement.classList.toggle("dark", resolvedDark());
}

function notify() {
    listeners.forEach((l) => l());
}

export function setTheme(next: Theme): void {
    theme = next;
    localStorage.setItem(KEY, next);
    notify();
    applyClass();
}

/** 当前该不该渲染为深色（按存储态，不等 DOM 类落定） */
export function isDark(): boolean {
    return resolvedDark();
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

/**
 * 切换主题：先更新存储态（图标/分段胶囊当场动起来），再播圆形展开。
 *
 * 圆形波：View Transition 圆形展开。`source` 的圆心写进 CSS 变量，圆半径取到视口最远角
 * （半路露出旧色会读成「没盖住」）→ 130ms 后才 `startViewTransition` 落 `.dark` 类。
 * 顺序不能反：VT 期间真 DOM 不绘制，一切 motion 动画都被快照定格，先起 VT 等于把触发件
 * 的反馈整个吞掉。
 *
 * 三条不播动画的路径：系统跟随被 OS 改动（没有用户动作当来源，凭空铺开读作 bug）、
 * 用户开了「减少动态效果」、深浅其实没变（如 system→light 而 OS 正是浅色）。一律直接切换。
 */
export function switchTheme(next: Theme, source?: HTMLElement | null): void {
    const wasDark = resolvedDark();

    theme = next;
    localStorage.setItem(KEY, next);
    notify();

    const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (!source || reduced) {
        applyClass();
        return;
    }

    const nowDark = resolvedDark();
    if (nowDark === wasDark) {
        applyClass();
        return;
    }

    const box = source.getBoundingClientRect();
    const x = box.left + box.width / 2;
    const y = box.top + box.height / 2;

    const doc = document as Document & {
        startViewTransition?: (cb: () => void) => unknown;
    };
    if (!doc.startViewTransition) {
        applyClass();
        return;
    }

    const style = document.documentElement.style;
    style.setProperty("--vt-x", `${Math.round(x)}px`);
    style.setProperty("--vt-y", `${Math.round(y)}px`);
    style.setProperty(
        "--vt-r",
        `${Math.round(Math.hypot(Math.max(x, innerWidth - x), Math.max(y, innerHeight - y)))}px`
    );

    window.clearTimeout(revealTimer);
    revealTimer = window.setTimeout(() => {
        doc.startViewTransition!(() => applyClass());
    }, PRE_DELAY);
}

// 模块载入即对齐一次 DOM，并在系统主题变化时刷新 system 态
applyClass();
media().addEventListener("change", () => {
    if (theme === "system") {
        notify();
        applyClass();
    }
});
