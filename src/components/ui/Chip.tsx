/**
 * 状态芯片与色调令牌：全站唯一的 tone 词表
 *
 * md：padding[4,10] gap5 r99 11/600（Home 徽章、Tasks 卡芯片）
 * sm：padding[3,8] r99 10/600（Convert 模组徽章）
 * xs：padding[2,8] r99 10/600（Errors 卡、Report 文件体积）
 */
import { type ReactNode } from "react";
import { type LucideIcon } from "lucide-react";
import { cn } from "@/lib/utils";

export type Tone =
    | "emerald"
    | "gold"
    | "redstone"
    | "accent"
    | "amethyst"
    | "diamond"
    | "muted";

const TONE_CHIP: Record<Tone, string> = {
    emerald: "bg-emerald-dim text-emerald",
    gold: "bg-gold-dim text-gold",
    redstone: "bg-redstone-dim text-redstone",
    accent: "bg-accent-dim text-accent",
    amethyst: "bg-surface-2 text-amethyst",
    diamond: "bg-surface-2 text-diamond",
    muted: "bg-surface-2 text-text-3",
};

/** 同套 tone 的纯文字版（计数行、变更明细图标） */
export const TONE_TEXT: Record<Tone, string> = {
    emerald: "text-emerald",
    gold: "text-gold",
    redstone: "text-redstone",
    accent: "text-accent",
    amethyst: "text-amethyst",
    diamond: "text-diamond",
    muted: "text-text-3",
};

const CHIP_SIZE = {
    md: "gap-[5px] px-2.5 py-1 text-[11px] leading-[16px]",
    sm: "gap-1 px-2 py-[3px] text-[10px] leading-[14px]",
    xs: "gap-1 px-2 py-0.5 text-[10px] leading-[14px]",
} as const;

export function ToneChip({
    tone,
    size = "md",
    icon: Icon,
    dot,
    mono,
    children,
    className,
}: {
    tone: Tone;
    size?: keyof typeof CHIP_SIZE;
    icon?: LucideIcon;
    /** 前置 6×6 圆点（Shift Rail / Tasks 运行中芯片） */
    dot?: boolean;
    /** 等宽字体 + 500 字重（Shift Rail 状态芯片） */
    mono?: boolean;
    children: ReactNode;
    className?: string;
}) {
    return (
        <span
            className={cn(
                "inline-flex shrink-0 items-center rounded-full font-semibold",
                CHIP_SIZE[size],
                mono && "font-mono font-medium",
                TONE_CHIP[tone],
                className
            )}
        >
            {dot && <span className="size-1.5 rounded-full bg-current" />}
            {Icon && <Icon className="size-2.5" />}
            {children}
        </span>
    );
}

/** 中性小徽章：$surface-2 底 10/600 $text-2（"服务端必装"等端标签）；square 时 r6 + 10/500（清单弹窗） */
export function TagChip({
    children,
    tone,
    square,
    outline,
    className,
}: {
    children: ReactNode;
    tone?: Tone;
    square?: boolean;
    outline?: boolean;
    className?: string;
}) {
    return (
        <span
            className={cn(
                "inline-flex shrink-0 items-center px-2 py-[3px] text-[10px] leading-[14px] font-semibold",
                square ? "rounded-md font-medium" : "rounded-full",
                tone ? TONE_CHIP[tone] : "bg-surface-2 text-text-2",
                outline && "border border-gold bg-transparent",
                className
            )}
        >
            {children}
        </span>
    );
}
