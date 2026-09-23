/**
 * 按钮族
 *
 * md：h36 padding[0,16] 13（primary 700 / outline 500）
 * sm：h32 padding[0,12] 12（primary 700 / outline 500）
 * xs：h28 padding[0,12] 11（Errors 卡内联按钮）
 *
 * title 一律不落到 DOM 上（系统灰泡不受样式管）：Btn/IconBtn 拿到它就补 aria-label、
 * 挂触发类，气泡由 Tip 画。所以调用方照旧写 title="复制全部日志" 即可。
 */
import { ChevronRight, type LucideIcon } from "lucide-react";
import { cn } from "@/lib/utils";
import { Tip, TIP_TRIGGER } from "./Tip";
import { HOVER_FILL } from "./HoverFill";

type BtnVariant = "primary" | "outline" | "ghost" | "danger";
type BtnSize = "md" | "sm" | "xs";

const BTN_VARIANT: Record<BtnVariant, string> = {
    primary: "bg-accent font-bold text-accent-ink hover:opacity-90",
    outline:
        "border border-stroke bg-transparent font-medium text-text-2 hover:bg-surface-2 hover:text-text-1",
    ghost: "bg-transparent font-medium text-text-2 hover:bg-surface-2 hover:text-text-1",
    danger: "border border-stroke bg-transparent font-semibold text-redstone hover:bg-redstone-dim",
};

const BTN_SIZE: Record<BtnSize, string> = {
    md: "h-9 px-4 text-[13px] leading-[20px] gap-[7px]",
    sm: "h-8 px-3 text-[12px] leading-[18px] gap-1.5",
    xs: "h-7 px-3 text-[11px] leading-[16px] gap-1",
};

const BTN_ICON: Record<BtnSize, string> = {
    md: "size-[13px]",
    sm: "size-3",
    xs: "size-3",
};

export function Btn({
    variant = "outline",
    size = "md",
    icon: Icon,
    full,
    className,
    title,
    "aria-label": ariaLabel,
    children,
    ...rest
}: React.ComponentProps<"button"> & {
    variant?: BtnVariant;
    size?: BtnSize;
    icon?: LucideIcon;
    /** 撑满父容器宽度（详情页右栏按钮列） */
    full?: boolean;
}) {
    return (
        <button
            aria-label={ariaLabel ?? title}
            className={cn(
                "inline-flex shrink-0 select-none items-center justify-center rounded-lg",
                HOVER_FILL,
                "disabled:pointer-events-none disabled:opacity-60",
                BTN_VARIANT[variant],
                BTN_SIZE[size],
                full && "w-full",
                title && TIP_TRIGGER,
                className
            )}
            {...rest}
        >
            {Icon && <Icon className={BTN_ICON[size]} />}
            {children}
            <Tip label={title} />
        </button>
    );
}

/** 方形图标按钮（28×28 弹窗关闭 / 34×26 标题栏控件之外的通用 ghost 控件） */
export function IconBtn({
    icon: Icon,
    className,
    title,
    "aria-label": ariaLabel,
    ...rest
}: React.ComponentProps<"button"> & { icon: LucideIcon }) {
    return (
        <button
            aria-label={ariaLabel ?? title}
            className={cn(
                "inline-flex h-8 w-9 shrink-0 items-center justify-center rounded-lg",
                "text-text-2 hover:bg-surface-2 hover:text-text-1",
                HOVER_FILL,
                "disabled:pointer-events-none disabled:opacity-40",
                title && TIP_TRIGGER,
                className
            )}
            {...rest}
        >
            <Icon className="size-3.5" />
            <Tip label={title} />
        </button>
    );
}

/** accent 文字链接：md 12/600（"查看全部 41 项剔除清单"）/ sm 11/600（Errors 卡日志链接） */
export function LinkBtn({
    size = "md",
    chevron,
    className,
    children,
    ...rest
}: React.ComponentProps<"button"> & { size?: "md" | "sm"; chevron?: boolean }) {
    return (
        <button
            className={cn(
                "inline-flex shrink-0 items-center gap-[5px] font-semibold text-accent hover:underline",
                size === "md"
                    ? "text-[12px] leading-[18px]"
                    : "text-[11px] leading-[16px]",
                className
            )}
            {...rest}
        >
            {children}
            {chevron && <ChevronRight className="size-3" />}
        </button>
    );
}
