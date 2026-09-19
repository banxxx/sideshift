/**
 * SideShift 设计规范公共原语（数值全部取自 SS.pen 各帧 dump，见 .design-ref/dump/*.txt）
 *
 * 标尺速查：
 *  - 卡片：$surface + $stroke 1px + r12 + padding 20；纵向 gap 按帧不同（Convert/Report 结果 14、
 *    Task 进度与日志 12、任务信息 10、弹窗 12）→ 用 Panel 的 gap 属性传入。
 *  - 卡内标题：13/600 $text-1；设置行标题：13/600 $text-1 + 11 $text-3 说明。
 *  - 控件高度：主按钮 36、次按钮/输入/选择/开关轨 32(开关本体 20)、分段轨 36（内项 30）。
 *  - 字号阶梯 10/11/12/13/14/16/22；徽章 r99，小元素 r6，控件 r8。
 * 所有页面一律用本文件组件拼装，避免各页手写 class 导致跨屏漂移。
 */
import { Dialog as DialogPrimitive } from "@base-ui/react/dialog";
import {
    ArrowLeft,
    Check,
    ChevronDown,
    ChevronRight,
    ChevronUp,
    Minus,
    Plus,
    Search,
    X,
    type LucideIcon,
} from "lucide-react";
import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { AnimatePresence, motion } from "motion/react";
import { cn } from "@/lib/utils";

/** 分段控件选中胶囊的滑动弹簧：短促、不回弹 */
export const SEG_PILL_SPRING = { type: "spring", stiffness: 420, damping: 36 } as const;

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

/* ---------------- 卡片容器：$surface + $stroke 1px + r12 + padding 20 ---------------- */

export function Panel({
    gap = 12,
    className,
    children,
}: {
    /** 卡内纵向间距（px）：设计稿按帧取 10 / 12 / 14 */
    gap?: number;
    className?: string;
    children: ReactNode;
}) {
    return (
        <section
            style={{ gap }}
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

/* ---------------- 状态芯片 ----------------
 * md：padding[4,10] gap5 r99 11/600（Home 徽章、Tasks 卡芯片）
 * sm：padding[3,8] r99 10/600（Convert 模组徽章）
 * xs：padding[2,8] r99 10/600（Errors 卡、Report 文件体积）
 */
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

/** 中性小徽章：$surface-2 底 10/600 $text-2（"客户端专属"等）；square 时 r6 + 10/500（剔除清单弹窗） */
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

/* ---------------- 按钮 ----------------
 * md：h36 padding[0,16] 13（primary 700 / outline 500）
 * sm：h32 padding[0,12] 12（primary 700 / outline 500）
 * xs：h28 padding[0,12] 11（Errors 卡内联按钮）
 */
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
            className={cn(
                "inline-flex shrink-0 select-none items-center justify-center rounded-lg transition-colors",
                "disabled:pointer-events-none disabled:opacity-60",
                BTN_VARIANT[variant],
                BTN_SIZE[size],
                full && "w-full",
                className
            )}
            {...rest}
        >
            {Icon && <Icon className={BTN_ICON[size]} />}
            {children}
        </button>
    );
}

/** 方形图标按钮（28×28 弹窗关闭 / 34×26 标题栏控件之外的通用 ghost 控件） */
export function IconBtn({
    icon: Icon,
    className,
    ...rest
}: React.ComponentProps<"button"> & { icon: LucideIcon }) {
    return (
        <button
            className={cn(
                "inline-flex h-8 w-9 shrink-0 items-center justify-center rounded-lg",
                "text-text-2 transition-colors hover:bg-surface-2 hover:text-text-1",
                "disabled:pointer-events-none disabled:opacity-40",
                className
            )}
            {...rest}
        >
            <Icon className="size-3.5" />
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

/* ---------------- 行排版 ---------------- */

/** 卡内单行：左 12/500 $text-1 标签 + 右控件（Convert 运行环境/启动参数各行） */
export function InlineRow({
    label,
    className,
    children,
}: {
    label: string;
    className?: string;
    children: ReactNode;
}) {
    return (
        <div className={cn("flex w-full items-center justify-between gap-4", className)}>
            <span className="min-w-0 text-[12px] leading-[18px] font-medium text-text-1">
                {label}
            </span>
            {children}
        </div>
    );
}

/** 设置分组标题：等宽 11/600 $text-3 + 1.2 字距（转换选项 / 网络 / 外观与关于） */
export function SectionTitle({ children }: { children: ReactNode }) {
    return (
        <span className="font-mono text-[11px] leading-[16px] font-semibold tracking-[1.2px] text-text-3">
            {children}
        </span>
    );
}

/** 设置行 padding[14,20]：左 13/600 $text-1 + 11 $text-3 说明（gap 2），右控件 */
export function SettingRow({
    label,
    desc,
    descMono,
    children,
}: {
    label: string;
    desc?: ReactNode;
    /** 说明行用等宽字体（版本行 "v0.1.0 · build 3 · Tauri 2"） */
    descMono?: boolean;
    children: ReactNode;
}) {
    return (
        <div className="flex w-full items-center justify-between gap-4 px-5 py-[14px]">
            <div className="flex min-w-0 flex-col gap-0.5">
                <span className="text-[13px] leading-[20px] font-semibold text-text-1">
                    {label}
                </span>
                {desc && (
                    <span
                        className={cn(
                            "text-[11px] leading-[16px] font-normal text-text-3",
                            descMono && "font-mono"
                        )}
                    >
                        {desc}
                    </span>
                )}
            </div>
            <div className="flex shrink-0 items-center gap-2">{children}</div>
        </div>
    );
}

/** 提示行：gap 6 + 12px 图标 + 等宽 11 $text-3（Convert 摘要卡 i-row） */
export function NoteRow({
    icon: Icon,
    children,
}: {
    icon: LucideIcon;
    children: ReactNode;
}) {
    return (
        <div className="flex items-center gap-1.5">
            <Icon className="size-3 shrink-0 text-text-3" />
            <span className="font-mono text-[11px] leading-[16px] font-normal text-text-3">
                {children}
            </span>
        </div>
    );
}

/* ---------------- Stepper：−|值|+ 高 32 ----------------
 * 卡内态（Convert）：$surface + $stroke 1px，分隔 1×20，值区 padding[0,14]
 * 行内态（Settings）：无底色，分隔 1×30，值区固定 40 宽
 */
export function Stepper({
    value,
    onChange,
    min = 1,
    max = 64,
    suffix,
    plain,
}: {
    value: number;
    onChange: (v: number) => void;
    min?: number;
    max?: number;
    suffix?: string;
    plain?: boolean;
}) {
    const step = (d: number) => onChange(Math.min(max, Math.max(min, value + d)));
    return (
        <div
            className={cn(
                "flex h-8 shrink-0 items-center rounded-lg",
                plain ? "border border-stroke" : "border border-stroke bg-surface"
            )}
        >
            <button
                className="inline-flex h-8 w-8 items-center justify-center text-text-2 transition-colors hover:bg-surface-2 disabled:opacity-40"
                disabled={value <= min}
                onClick={() => step(-1)}
            >
                <Minus className="size-3" />
            </button>
            <span className={cn("w-px bg-stroke", plain ? "h-[30px]" : "h-5")} />
            <span
                className={cn(
                    "inline-flex h-8 items-center justify-center font-mono text-[12px] leading-[18px] font-semibold text-text-1",
                    plain ? "w-10" : "px-3.5"
                )}
            >
                {value}
                {suffix && <span className="ml-1">{suffix}</span>}
            </span>
            <span className={cn("w-px bg-stroke", plain ? "h-[30px]" : "h-5")} />
            <button
                className="inline-flex h-8 w-8 items-center justify-center text-text-2 transition-colors hover:bg-surface-2 disabled:opacity-40"
                disabled={value >= max}
                onClick={() => step(1)}
            >
                <Plus className="size-3" />
            </button>
        </div>
    );
}

/* ---------------- 开关：36×20（卡内）/ 38×22（设置行），滑块 16 白色 ---------------- */

export function Toggle({
    checked,
    onChange,
    size = "sm",
    disabled,
}: {
    checked: boolean;
    onChange: (v: boolean) => void;
    size?: "sm" | "md";
    disabled?: boolean;
}) {
    return (
        <button
            role="switch"
            aria-checked={checked}
            disabled={disabled}
            onClick={() => onChange(!checked)}
            className={cn(
                "flex shrink-0 items-center rounded-full p-0.5 transition-colors",
                "disabled:pointer-events-none disabled:opacity-60",
                size === "sm" ? "h-5 w-9" : "h-[22px] w-[38px] p-[3px]",
                checked ? "justify-end bg-accent" : "justify-start border border-stroke bg-surface-2"
            )}
        >
            <span className="size-4 shrink-0 rounded-full bg-white" />
        </button>
    );
}

/* ---------------- 分段 Tab：轨 h36 gap2 padding3 $surface-2 r8；内项 h30 padding[0,12] r6 ----------------
 * 选中：$surface + $stroke 1px + 12/600 $text-1；未选中：12/500 $text-3
 * 设计稿把「标签 + 计数」写成一个文本节点（如 "剔除 41"），这里同样拼接为单节点。
 */
export function SegTabs<T extends string>({
    items,
    value,
    onChange,
    className,
}: {
    items: Array<{ key: T; label: string; count?: number; icon?: LucideIcon }>;
    value: T;
    onChange: (k: T) => void;
    className?: string;
}) {
    // 选中态抽成一颗 layoutId 胶囊：切 Tab 时它在三项之间滑动，而不是瞬移换底色
    const pillId = useId();
    return (
        <div
            className={cn(
                "flex h-9 shrink-0 items-center gap-0.5 rounded-lg bg-surface-2 p-[3px]",
                className
            )}
        >
            {items.map((it) => {
                const active = it.key === value;
                const Icon = it.icon;
                return (
                    <button
                        key={it.key}
                        onClick={() => onChange(it.key)}
                        className={cn(
                            "relative inline-flex h-[30px] items-center justify-center rounded-md px-3 transition-colors",
                            active
                                ? "text-[12px] leading-[18px] font-semibold text-text-1"
                                : "text-[12px] leading-[18px] font-medium text-text-3 hover:text-text-2"
                        )}
                    >
                        {active && (
                            <motion.span
                                layoutId={`${pillId}-seg-pill`}
                                transition={SEG_PILL_SPRING}
                                className="absolute inset-0 rounded-md border border-stroke bg-surface"
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

/* ---------------- 可搜索下拉 ----------------
 * 触发框（Convert 卡内）：h32 padding[0,10] 两端对齐 $surface + $stroke 1px r8；
 *   值 等宽 12/500 $text-1，chevron-down 12 $text-3；展开态描边换成 $accent 1.5px + chevron-up $accent
 * 触发框（plain，Settings 行内）：$bg-app 底，值 等宽 11 $text-2
 * 面板：padding 6 gap2 $surface + $stroke 1px r8；搜索行 h26；分组标 10 $text-3；
 *   选项 h26 padding[0,8] r6，选中 $surface-2 + 12/600 $accent + check 12
 */
export interface SelectOption {
    value: string;
    label: string;
    recommended?: boolean;
    group?: string;
}

export function SearchSelect({
    label,
    value,
    options,
    onChange,
    plain,
    className,
}: {
    label?: string;
    value: string;
    options: SelectOption[];
    onChange: (v: string) => void;
    plain?: boolean;
    className?: string;
}) {
    const [open, setOpen] = useState(false);
    const [query, setQuery] = useState("");
    const boxRef = useRef<HTMLDivElement>(null);

    // 点击外部关闭
    useEffect(() => {
        if (!open) return;
        const onDown = (e: MouseEvent) => {
            if (!boxRef.current?.contains(e.target as Node)) setOpen(false);
        };
        window.addEventListener("mousedown", onDown);
        return () => window.removeEventListener("mousedown", onDown);
    }, [open]);

    const filtered = options.filter(
        (o) =>
            !query ||
            o.label.toLowerCase().includes(query.toLowerCase()) ||
            o.value.toLowerCase().includes(query.toLowerCase())
    );
    const current = options.find((o) => o.value === value);

    return (
        <div ref={boxRef} className={cn("relative flex flex-col gap-1.5", className)}>
            {label && (
                <span className="text-[11px] leading-[16px] font-normal text-text-3">{label}</span>
            )}
            <button
                onClick={() => setOpen((v) => !v)}
                className={cn(
                    "flex h-8 w-full items-center justify-between gap-2 rounded-lg border px-2.5 transition-colors",
                    plain ? "border-stroke bg-bg-app" : "border-stroke bg-surface",
                    !plain && open && "border-[1.5px] border-accent"
                )}
            >
                <span
                    className={cn(
                        "truncate font-mono",
                        plain
                            ? "text-[11px] leading-[16px] font-normal text-text-2"
                            : "text-[12px] leading-[18px] font-medium text-text-1"
                    )}
                >
                    {current?.label ?? value}
                </span>
                {open ? (
                    <ChevronUp className="size-3 shrink-0 text-accent" />
                ) : (
                    <ChevronDown className="size-3 shrink-0 text-text-3" />
                )}
            </button>
            <AnimatePresence>
                {open && (
                    <motion.div
                        // 从触发框向下"抽出"：顶部为原点做纵向缩放 + 轻微位移 + 淡入淡出
                        initial={{ opacity: 0, y: -6, scaleY: 0.9 }}
                        animate={{ opacity: 1, y: 0, scaleY: 1 }}
                        exit={{ opacity: 0, y: -6, scaleY: 0.9 }}
                        transition={{ duration: 0.16, ease: [0.16, 1, 0.3, 1] }}
                        className="absolute top-full right-0 left-0 z-30 mt-1 flex min-w-[220px] origin-top flex-col gap-0.5 rounded-lg border border-stroke bg-surface p-1.5 shadow-lg"
                    >
                        <div className="flex h-[26px] shrink-0 items-center gap-1.5 rounded-md px-2">
                            <Search className="size-3 shrink-0 text-text-3" />
                            <input
                                value={query}
                                onChange={(e) => setQuery(e.target.value)}
                                placeholder="搜索版本…"
                                className="min-w-0 flex-1 bg-transparent text-[11px] text-text-1 outline-none placeholder:text-text-3"
                            />
                        </div>
                        <div className="h-px w-full bg-stroke" />
                        <div className="flex max-h-[212px] flex-col gap-0.5 overflow-auto">
                            {filtered.map((o, i) => {
                                const active = o.value === value;
                                // 分组标题只在该组第一行出现（同组连续排列），不再逐行重复
                                const showGroup =
                                    !!o.group && filtered[i - 1]?.group !== o.group;
                                return (
                                    <div key={o.value} className="flex flex-col">
                                        {showGroup && (
                                            <span className="px-2 pt-1 pb-0.5 text-[10px] leading-[14px] font-normal text-text-3">
                                                {o.group}
                                            </span>
                                        )}
                                        <button
                                            onClick={() => {
                                                onChange(o.value);
                                                setOpen(false);
                                                setQuery("");
                                            }}
                                            className={cn(
                                                "flex h-[26px] w-full items-center justify-between gap-2 rounded-md px-2 transition-colors",
                                                active ? "bg-surface-2" : "hover:bg-surface-2"
                                            )}
                                        >
                                            <span
                                                className={cn(
                                                    "truncate font-mono text-[12px] leading-[18px]",
                                                    active
                                                        ? "font-semibold text-accent"
                                                        : "font-normal text-text-1"
                                                )}
                                            >
                                                {o.label}
                                            </span>
                                            {active && (
                                                <Check className="size-3 shrink-0 text-accent" />
                                            )}
                                        </button>
                                    </div>
                                );
                            })}
                            {filtered.length === 0 && (
                                <span className="px-2 py-3 text-center text-[11px] text-text-3">
                                    无匹配版本
                                </span>
                            )}
                        </div>
                    </motion.div>
                )}
            </AnimatePresence>
        </div>
    );
}

/* ---------------- 勾选框：16×16 r4（Convert 模组行） ----------------
 * 选中 $accent + check 10 $accent-ink；未选 $surface + $stroke；待确认未选 $surface + $gold
 */
export function CheckBox({
    checked,
    review,
    onChange,
}: {
    checked: boolean;
    review?: boolean;
    onChange: (v: boolean) => void;
}) {
    return (
        <button
            role="checkbox"
            aria-checked={checked}
            onClick={() => onChange(!checked)}
            className={cn(
                "flex size-4 shrink-0 items-center justify-center rounded transition-colors",
                checked
                    ? "bg-accent"
                    : review
                      ? "border border-gold bg-surface"
                      : "border border-stroke bg-surface hover:border-text-3"
            )}
        >
            {checked && <Check className="size-2.5 text-accent-ink" strokeWidth={3} />}
        </button>
    );
}

/* ---------------- 进度条：h6 轨道 $surface-2 r99 + 色条 ---------------- */

export function Bar({
    percent,
    className,
    fillClass,
}: {
    percent: number;
    className?: string;
    fillClass: string;
}) {
    return (
        <div className={cn("h-1.5 w-full overflow-hidden rounded-full bg-surface-2", className)}>
            <div
                className={cn("h-1.5 rounded-full transition-[width]", fillClass)}
                style={{ width: `${Math.max(0, Math.min(100, percent))}%` }}
            />
        </div>
    );
}

/* ---------------- 计数行 / info 行 / 元数据 ---------------- */

const TONE_TEXT: Record<Tone, string> = {
    emerald: "text-emerald",
    gold: "text-gold",
    redstone: "text-redstone",
    accent: "text-accent",
    amethyst: "text-amethyst",
    diamond: "text-diamond",
    muted: "text-text-3",
};

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
            {/* 定高裁剪窗：计数变化时旧数上滑退场、新数下方升入 */}
            <span className="flex h-[20px] items-center overflow-hidden">
                <AnimatePresence mode="popLayout" initial={false}>
                    <motion.span
                        key={count}
                        initial={{ y: 16, opacity: 0 }}
                        animate={{ y: 0, opacity: 1 }}
                        exit={{ y: -16, opacity: 0 }}
                        transition={{ type: "spring", stiffness: 420, damping: 32 }}
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
 */
export function ChangeRow({
    icon: Icon,
    tone,
    title,
    sub,
    count,
}: {
    icon: LucideIcon;
    tone: Tone;
    title: string;
    sub: string;
    count: number;
}) {
    return (
        <div className="flex w-full items-center gap-2.5">
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
        </div>
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
                "flex min-w-0 flex-1 flex-col gap-0.5 rounded-lg bg-surface-2 px-3 py-2.5",
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
                    {value ?? "—"}
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

/* ---------------- 输入框：h32 r8；卡内 $surface-2 无描边，plain 为 $bg-app + $stroke ---------------- */

export function TextInput({
    icon: Icon,
    plain,
    className,
    ...rest
}: React.ComponentProps<"input"> & { icon?: LucideIcon; plain?: boolean }) {
    return (
        <div
            className={cn(
                "flex h-8 shrink-0 items-center gap-[7px] rounded-lg px-2.5",
                plain ? "border border-stroke bg-bg-app" : "bg-surface-2",
                "focus-within:ring-1 focus-within:ring-accent",
                className
            )}
        >
            {Icon && <Icon className="size-3 shrink-0 text-text-3" />}
            <input
                className={cn(
                    "min-w-0 flex-1 bg-transparent outline-none placeholder:text-text-3",
                    plain
                        ? "font-mono text-[11px] leading-[16px] text-text-2"
                        : "text-[13px] leading-[20px] text-text-1"
                )}
                {...rest}
            />
        </div>
    );
}

/* ---------------- 弹窗壳：$surface + $stroke 1px r12 padding20 gap12 ----------------
 * 头：标题 14/600 + 副标 11 $text-3（gap3），可选 40×40 图标盒（Mod Detail），右侧 28×28 返回/关闭
 * 脚：1px $stroke 分隔 + 左摘要文本 11 $text-3 + 右按钮组（gap8）
 */
export function ModalShell({
    open,
    onClose,
    width,
    height,
    title,
    sub,
    icon: Icon,
    back,
    children,
    footerNote,
    footerActions,
    persistent,
}: {
    open: boolean;
    onClose: () => void;
    width: number;
    height?: number;
    title: string;
    sub?: string;
    icon?: LucideIcon;
    back?: () => void;
    children: ReactNode;
    footerNote?: string;
    footerActions?: ReactNode;
    /** 防误触：点遮罩/按 Esc 不关闭，只能走按钮（目录勾选弹窗用） */
    persistent?: boolean;
}) {
    return (
        <DialogPrimitive.Root
            open={open}
            onOpenChange={(o) => {
                if (!o && !persistent) onClose();
            }}
        >
            <DialogPrimitive.Portal>
                {/* base-ui 在出入场过渡期挂 data-starting/ending-style，配 CSS 过渡做淡入+微缩放 */}
                <DialogPrimitive.Backdrop
                    className={cn(
                        "fixed inset-0 z-50 bg-black/50 transition-opacity duration-200",
                        "data-[starting-style]:opacity-0 data-[ending-style]:opacity-0"
                    )}
                />
                <DialogPrimitive.Popup
                    className={cn(
                        "fixed top-1/2 left-1/2 z-50 flex -translate-x-1/2 -translate-y-1/2 flex-col gap-3 rounded-[12px] border border-stroke bg-surface p-5 outline-none",
                        "transition-[opacity,scale] duration-200",
                        "data-[starting-style]:opacity-0 data-[starting-style]:scale-[0.96]",
                        "data-[ending-style]:opacity-0 data-[ending-style]:scale-[0.96]"
                    )}
                    style={{ width, height }}
                >
                    <div className="flex w-full items-center justify-between gap-2.5">
                        {Icon && (
                            <span className="flex size-10 shrink-0 items-center justify-center rounded-lg bg-surface-2">
                                <Icon className="size-5 text-accent" />
                            </span>
                        )}
                        <div className="flex min-w-0 flex-1 flex-col gap-[3px]">
                            <DialogPrimitive.Title className="truncate text-[14px] leading-[20px] font-semibold text-text-1">
                                {title}
                            </DialogPrimitive.Title>
                            {sub && (
                                <span className="truncate text-[11px] leading-[16px] font-normal text-text-3">
                                    {sub}
                                </span>
                            )}
                        </div>
                        <div className="flex shrink-0 items-center gap-2">
                            {back && (
                                <button
                                    onClick={back}
                                    title="返回"
                                    className="flex size-7 items-center justify-center rounded-lg border border-stroke bg-surface text-text-2 transition-colors hover:bg-surface-2"
                                >
                                    <ArrowLeft className="size-3.5" />
                                </button>
                            )}
                            <button
                                onClick={onClose}
                                title="关闭"
                                className="flex size-7 items-center justify-center rounded-lg border border-stroke text-text-2 transition-colors hover:bg-surface-2"
                            >
                                <X className="size-3.5" />
                            </button>
                        </div>
                    </div>

                    {children}

                    {(footerNote || footerActions) && (
                        <div className="flex w-full flex-col gap-3">
                            <Divider hard />
                            <div className="flex w-full items-center justify-between gap-2.5">
                                <span className="text-[11px] leading-[16px] font-normal text-text-3">
                                    {footerNote}
                                </span>
                                <div className="flex items-center gap-2">{footerActions}</div>
                            </div>
                        </div>
                    )}
                </DialogPrimitive.Popup>
            </DialogPrimitive.Portal>
        </DialogPrimitive.Root>
    );
}

/** 弹窗内搜索/筛选行：h32 padding[0,10] gap8 r8（描边或 $surface-2 底由 className 决定） */
export function SearchBox({
    value,
    onChange,
    placeholder,
    className,
}: {
    value: string;
    onChange: (v: string) => void;
    placeholder: string;
    className?: string;
}) {
    return (
        <div
            className={cn(
                "flex h-8 w-full shrink-0 items-center gap-2 rounded-lg px-2.5",
                className
            )}
        >
            <Search className="size-3.5 shrink-0 text-text-3" />
            <input
                value={value}
                onChange={(e) => onChange(e.target.value)}
                placeholder={placeholder}
                className="min-w-0 flex-1 bg-transparent text-[12px] leading-[18px] text-text-1 outline-none placeholder:text-text-3"
            />
            {value && (
                <button
                    title="清空搜索"
                    onClick={() => onChange("")}
                    className="flex size-4 shrink-0 items-center justify-center rounded text-text-3 transition-colors hover:bg-surface-2 hover:text-text-1"
                >
                    <X className="size-3" />
                </button>
            )}
        </div>
    );
}

/** 弹窗列表行容器：padding[8,4] gap10 两端对齐（选中/悬停底色由 className 决定） */
export function ListRow({ className, ...rest }: React.ComponentProps<"div">) {
    return (
        <div
            className={cn(
                "flex w-full items-center gap-2.5 rounded-lg px-1 py-2 transition-colors",
                className
            )}
            {...rest}
        />
    );
}
