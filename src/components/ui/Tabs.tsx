/**
 * 分段 Tab：轨 h36 gap2 padding3 $surface-2 r8；内项 h30 padding[0,12] r6
 * 选中：$surface + $stroke 1px + 12/600 $text-1；未选中：12/500 $text-3
 * 设计稿把「标签 + 计数」写成一个文本节点（如 "剔除 41"），这里同样拼接为单节点。
 */
import { useId } from "react";
import { motion } from "motion/react";
import { type LucideIcon } from "lucide-react";
import { cn } from "@/lib/utils";
import { HOVER_FILL } from "./HoverFill";
import { SEG_PILL } from "@/lib/springs";

export function SegTabs<T extends string>({
    items,
    value,
    onChange,
    size = "md",
    className,
}: {
    items: Array<{ key: T; label: string; count?: number; icon?: LucideIcon }>;
    value: T;
    /**
     * 第二参是**被点的那颗按钮**：主题切换拿它的矩形当圆形展开的圆心，
     * 只接 key 的调用方照旧写一参函数就行（TS 允许实现少收参数）。
     */
    onChange: (k: T, el: HTMLButtonElement) => void;
    /** md = 页面级页签（轨道 h36）；sm = 弹窗行内筛选（轨道 h28），别处别拿它当小号页签用 */
    size?: "md" | "sm";
    className?: string;
}) {
    // 选中态抽成一颗 layoutId 胶囊：切 Tab 时它在三项之间滑动，而不是瞬移换底色
    const pillId = useId();
    return (
        <div
            className={cn(
                "flex shrink-0 items-center gap-0.5 rounded-lg bg-surface-2",
                size === "sm" ? "h-7 p-[2px]" : "h-9 p-[3px]",
                className
            )}
        >
            {items.map((it) => {
                const active = it.key === value;
                const Icon = it.icon;
                return (
                    <button
                        key={it.key}
                        onClick={(e) => onChange(it.key, e.currentTarget)}
                        className={cn(
                            "relative inline-flex shrink-0 items-center justify-center rounded-md",
                            HOVER_FILL,
                            size === "sm"
                                ? "h-[22px] px-2 text-[11px] leading-[16px]"
                                : "h-[30px] px-3 text-[12px] leading-[18px]",
                            active ? "font-semibold text-text-1" : "font-medium text-text-3 hover:text-text-2"
                        )}
                    >
                        {active && (
                            <motion.span
                                layoutId={`${pillId}-seg-pill`}
                                transition={SEG_PILL}
                                className={cn(
                                    "absolute inset-0 rounded-md border border-stroke bg-surface",
                                    HOVER_FILL
                                )}
                            />
                        )}
                        {/* tabular-nums：数字位宽一致，计数变化时同一 Tab 不再横向抽动 */}
                        <span className="relative z-[1] inline-flex items-center justify-center gap-[5px] tabular-nums">
                            {Icon && (
                                <Icon
                                    className={cn(
                                        "size-[11px]",
                                        active ? "text-emerald" : "text-text-3"
                                    )}
                                />
                            )}
                            {it.count === undefined ? it.label : `${it.label} ${it.count}`}
                        </span>
                    </button>
                );
            })}
        </div>
    );
}
