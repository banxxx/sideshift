/**
 * 计数 / 信息 / 元数据展示件 + 进度条
 */
import { ChevronDown, type LucideIcon } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { cn } from "@/lib/utils";
import { TONE_TEXT, type Tone } from "./Chip";
import { HOVER_FILL } from "./HoverFill";
import { COUNT_ROLL } from "@/lib/springs";

/* ---------------- 进度条：h6 轨道 + 色条（v5：轨道列表档 $bg-app、详情档 $surface-2；
 * 色条带 8/15 双圈柔光，外圈再散就成雾了。cr 两档（列表 3 / 详情 99）在 h6 上都是「半个高」，画出来同形 ⇒ 不分叉。）
 * 轨上**不挂 overflow**：色条与轨等高、宽度封顶 100%，越不出盒子，唯一被裁掉的正是设计要的那圈光。 ---------------- */

export function Bar({
    percent,
    className,
    fillClass,
    flow,
}: {
    percent: number;
    className?: string;
    fillClass: string;
    /** 进行中：已完成段扫一道光（口径见 App.css 的 `.bar-flow`）。不给就不动，静止的条是诚实的 */
    flow?: boolean;
}) {
    return (
        <div className={cn("h-1.5 w-full rounded-full bg-surface-2", className)}>
            <div
                className={cn(
                    "h-1.5 rounded-full transition-[width]",
                    fillClass,
                    flow && "bar-flow",
                    // 0% 时色条没有面积、但那圈光还在 ⇒ 会留一团雾，直接把投影关掉
                    percent <= 0 && "shadow-none"
                )}
                style={{ width: `${Math.max(0, Math.min(100, percent))}%` }}
            />
        </div>
    );
}

/** 摘要计数行：左 12 normal $text-2 + 右 等宽 14/600 状态色 */
export function CountRow({
    label,
    count,
    tone,
}: {
    label: string;
    count: number;
    tone: Tone;
}) {
    return (
        <div className="flex w-full items-center justify-between">
            <span className="text-[12px] leading-[18px] font-normal text-text-2">{label}</span>
            {/* 定高裁剪窗：计数变化时旧数上滑退场、新数下方升入。
                `relative` 是必需的，不是装饰：popLayout 给退场那层写的是 `position:absolute !important`
                + 按 `offsetParent` 量的 top/left。这一格原先不定位 ⇒ offsetParent 一路找到文档根，
                页面向下滚过之后那个坐标就不再指着这一行——数字当场飘到右边别处（模板摘要/转换摘要同一条路径）。 */}
            <span className="relative flex h-[20px] items-center overflow-hidden">
                <AnimatePresence mode="popLayout" initial={false}>
                    <motion.span
                        key={count}
                        initial={{ y: 16, opacity: 0 }}
                        animate={{ y: 0, opacity: 1 }}
                        exit={{ y: -16, opacity: 0 }}
                        transition={COUNT_ROLL}
                        className={cn(
                            "font-mono text-[14px] leading-[20px] font-semibold",
                            TONE_TEXT[tone]
                        )}
                    >
                        {count}
                    </motion.span>
                </AnimatePresence>
            </span>
        </div>
    );
}

/**
 * 报告「变更明细」行：14px 图标 + 12/500 标题 + 11 $text-3 依据 + 右侧等宽 13/600 计数。
 * 与 CountRow（Convert 摘要，无图标/依据，计数 14）刻意分开，避免两屏互相牵制。
 * 给了 onClick 就整行可展开（右侧露一枚旋转的 V），不另设第二个点击区。
 */
export function ChangeRow({
    icon: Icon,
    tone,
    title,
    sub,
    count,
    onClick,
    open,
}: {
    icon: LucideIcon;
    tone: Tone;
    title: string;
    sub: string;
    count: number;
    onClick?: () => void;
    open?: boolean;
}) {
    const row = (
        <>
            <Icon className={cn("size-3.5 shrink-0", TONE_TEXT[tone])} />
            <span className="shrink-0 text-[12px] leading-[18px] font-medium text-text-1">
                {title}
            </span>
            <span className="min-w-0 flex-1 truncate text-[11px] leading-[16px] font-normal text-text-3">
                {sub}
            </span>
            <span
                className={cn(
                    "shrink-0 text-right font-mono text-[13px] leading-[20px] font-semibold",
                    TONE_TEXT[tone]
                )}
            >
                {count}
            </span>
            {onClick && (
                <ChevronDown
                    className={cn(
                        "size-3 shrink-0 text-text-3 transition-transform duration-200",
                        open && "rotate-180"
                    )}
                />
            )}
        </>
    );
    const shell = "flex w-full items-center gap-2.5 -mx-1 px-1 rounded-lg";
    return onClick ? (
        <button
            onClick={onClick}
            aria-expanded={open}
            className={cn(shell, "hover:bg-surface-2", HOVER_FILL)}
        >
            {row}
        </button>
    ) : (
        <div className={shell}>{row}</div>
    );
}

/** 信息行：左 11 $text-3 + 右 等宽 11/500 $text-1（任务信息卡 / 报告输出卡） */
export function InfoRow({
    label,
    value,
    valueClass,
}: {
    label: string;
    value: string;
    valueClass?: string;
}) {
    return (
        <div className="flex w-full items-center justify-between gap-3">
            <span className="shrink-0 text-[11px] leading-[16px] font-normal text-text-3">
                {label}
            </span>
            <span
                className={cn(
                    "truncate font-mono text-[11px] leading-[16px] font-medium text-text-1",
                    valueClass
                )}
            >
                {value}
            </span>
        </div>
    );
}

/** 元数据格：$surface-2 底 r8 padding[10,12] gap2；标 11 $text-3 + 值 等宽 13/600 */
export function MetaCell({
    label,
    value,
    skeleton,
    valueClass,
    className,
}: {
    label: string;
    value?: string;
    /** 解析中：显示灰条占位 */
    skeleton?: boolean;
    valueClass?: string;
    className?: string;
}) {
    return (
        <div
            className={cn(
                "flex min-w-0 flex-1 flex-col gap-0.5 rounded-md bg-veil px-3 py-2.5",
                className
            )}
        >
            <span className="text-[11px] leading-[16px] font-normal text-text-3">{label}</span>
            {skeleton ? (
                <span className="mt-1 h-4 w-14 rounded bg-stroke" />
            ) : (
                <span
                    className={cn(
                        "truncate font-mono text-[13px] leading-[20px] font-semibold",
                        value ? "text-text-1" : "text-text-3",
                        valueClass
                    )}
                >
                    {value || "—"}
                </span>
            )}
        </div>
    );
}

/** 报告页横向元数据列：标 10 $text-3 + 值 等宽 11/500 $text-1（gap 4，列间 24） */
export function MiniMeta({ label, value }: { label: string; value: string }) {
    return (
        <div className="flex flex-col gap-1">
            <span className="text-[10px] leading-[14px] font-normal text-text-3">{label}</span>
            <span className="font-mono text-[11px] leading-[16px] font-medium text-text-1">
                {value}
            </span>
        </div>
    );
}
