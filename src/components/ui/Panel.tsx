/**
 * 页面骨架：页头 + 卡片容器 + 卡头 + 分隔线
 *
 * 标尺（数值取自 SS.pen 各帧 dump，见 .design-ref/dump/*.txt）：
 *  - 卡片：$surface + $stroke 1px + r12 + padding 20；纵向 gap 按帧不同（Convert/Report 结果 14、
 *    Task 进度与日志 12、任务信息 10、弹窗 12）→ 用 Panel 的 gap 属性传入。
 *  - 卡内标题：13/600 $text-1。
 */
import { type ReactNode } from "react";
import { cn } from "@/lib/utils";

/* ---------------- 页头 ----------------
 * 默认（Home/Convert/Task/Report）：纵向 gap 6，H1 22/700 + sub 13 $text-2
 * compact（Settings/Tasks）：纵向 gap 4；subTone="mono" 时 sub 为 11 等宽 $text-3
 * right：Tasks 头右侧的分段筛选
 */
export function PageHeader({
    title,
    sub,
    subTone = "body",
    compact,
    right,
}: {
    title: ReactNode;
    sub?: ReactNode;
    subTone?: "body" | "mono";
    compact?: boolean;
    right?: ReactNode;
}) {
    return (
        <header className="flex w-full items-center justify-between gap-5">
            <div className={cn("flex min-w-0 flex-col", compact ? "gap-1" : "gap-1.5")}>
                <h1 className="font-heading text-[22px] leading-[28px] font-bold text-text-1">
                    {title}
                </h1>
                {sub != null &&
                    (subTone === "mono" ? (
                        <p className="font-mono text-[11px] leading-[16px] font-normal text-text-3">
                            {sub}
                        </p>
                    ) : (
                        <p
                            className={cn(
                                "font-normal text-text-2",
                                compact
                                    ? "text-[12px] leading-[18px]"
                                    : "text-[13px] leading-[20px]"
                            )}
                        >
                            {sub}
                        </p>
                    ))}
            </div>
            {right}
        </header>
    );
}

/* ---------------- 卡片容器 ---------------- */

export function Panel({
    gap = 12,
    padY,
    className,
    children,
}: {
    /** 卡内纵向间距（px）：设计稿按帧取 10 / 12 / 14 */
    gap?: number;
    /**
     * 上下内边距（px），不给就是 `p-5` 的 20。
     *
     * 只给「收起后只剩一行表头」的那种卡用：`PanelHead` 的高度是右侧那颗按钮顶出来的（`FoldBtn` 28px），
     * 上下各 20 就是一行字顶着 70px 的框。走 inline `paddingBlock` 而不是传 `className="py-3"`，
     * 是为了不跟 `p-5` 抢 Tailwind 的输出顺序——内联样式一定压过类。左右一律不动，横向标尺全站统一。
     */
    padY?: number;
    className?: string;
    children: ReactNode;
}) {
    return (
        <section
            style={{ gap, ...(padY != null ? { paddingBlock: padY } : null) }}
            className={cn(
                "flex flex-col rounded-[12px] border border-stroke bg-surface p-5",
                className
            )}
        >
            {children}
        </section>
    );
}

/**
 * 卡片头行：13/600 $text-1 标题 + 右侧插槽（芯片/分段 Tab）。
 * inline=true 时标题与右侧内容左对齐紧挨（Task 卡「转换进度 + 状态芯片」），
 * 否则两端对齐（Convert 卡「模组方案 + 分段 Tab」）。
 */
export function PanelHead({
    title,
    right,
    inline,
}: {
    title: string;
    right?: ReactNode;
    inline?: boolean;
}) {
    return (
        <div
            className={cn(
                "flex w-full items-center",
                inline ? "gap-2" : "justify-between gap-3"
            )}
        >
            <span className="text-[13px] leading-[20px] font-semibold text-text-1">
                {title}
            </span>
            {right}
        </div>
    );
}

/** 1px 分隔线：soft=卡内弱化分隔（$stroke-soft），hard=弹窗页脚上方（$stroke） */
export function Divider({ hard }: { hard?: boolean } = {}) {
    return <div className={cn("h-px w-full", hard ? "bg-stroke" : "bg-stroke-soft")} />;
}
