/**
 * 悬停提示气泡 Tip
 *
 * 替掉原生 `title`：那个灰泡由系统绘制，深浅色、字号、圆角、出现时机都不归样式系统管，
 * 是界面上唯一不受设计令牌约束的一块。
 *
 * 用法：触发元素挂 `TIP_TRIGGER`，气泡作为它的**子节点**渲染（不套 wrapper，
 * 免得给 flex 行多塞一层盒子的同时把 gap 也算到它头上）。
 *
 * 具名悬停组 `group/tip` 是必需的，不能用普通 `group`：方案行移除、提示条关闭这类按钮
 * 本身就靠父级的 `group-hover` 淡入，同名组会让「悬停整行」误点亮按钮气泡。
 *
 * 隐藏只用 `opacity-0`，所以气泡**一直在布局里**：绝对定位件的边框盒会算进最近滚动祖先的
 * 可滚溢出，越过视口下缘就是把整窗撑出一道常驻滚动条（标题栏跟着滚）。
 * 于是两条硬约束：贴底触发件用 `side="top"` 朝上长；外壳（App 根）用 `overflow-hidden`
 * 把文档级滚动钉死，滚动只归 `main.page-scroll`。
 *
 * 出现节奏全交给 transition-delay——悬停 380ms 才淡入（横扫标题栏不会连闪一串气泡），
 * 移开时 delay 归零、120ms 淡出。全程无状态、无 JS 计时器，
 * 因此原生文件框冻结指针事件那类卡死（悬停态陷阱）在这里没有立足点。
 */
import { cn } from "@/lib/utils";

/** 触发元素要挂的类：定位参照 + 具名悬停组 */
export const TIP_TRIGGER = "group/tip relative";

export function Tip({ label, align = "end", side = "bottom", wide }: {
    label?: string;
    /**
     * 贴哪一边。默认往左长（end）：本项目的提示触发件几乎都是行右端的动作簇，
     * 居中会让气泡顶出 `main` 的滚动区、被裁的同时还撑出一道横向滚动条。
     * 整宽文本格（路径/当前动作）用 start，贴着文字左缘铺出去才对得上。
     */
    align?: "center" | "start" | "end";
    /**
     * 长在哪一侧。**贴窗底/贴容器底的触发件必须用 "top"**（侧栏主题钮、提示卡 ×）：
     * 气泡是常驻 DOM（只靠 opacity 藏），`top-full` 会把溢出算给最近的滚动祖先，
     * 越过视口下缘就是一道拉不掉的窗口滚动条。
     */
    side?: "bottom" | "top";
    /** 长串（路径 / 文件名）用：等宽、更宽、允许在任意字符处断行 */
    wide?: boolean;
}) {
    if (!label) return null;
    return (
        <span
            aria-hidden
            className={cn(
                // 底色/描边/圆角与 SearchSelect 面板同源，弹层语言保持一致
                "pointer-events-none absolute z-[60] w-max rounded-md",
                side === "bottom" ? "top-full mt-1.5" : "bottom-full mb-1.5",
                "border border-stroke bg-surface px-2 py-[3px] text-left",
                "text-[11px] leading-[15px] font-normal whitespace-normal text-text-2 shadow-md",
                "opacity-0 delay-0 transition-opacity duration-[120ms] ease-out",
                "group-hover/tip:opacity-100 group-hover/tip:delay-[380ms]",
                "group-focus-visible/tip:opacity-100",
                wide
                    ? "max-w-[360px] font-mono break-all"
                    : "max-w-[220px] break-words",
                align === "center" && "left-1/2 -translate-x-1/2",
                align === "start" && "left-0",
                align === "end" && "right-0"
            )}
        >
            {label}
        </span>
    );
}
