/**
 * 装机向导（安装壳 / 卸载壳）的原子件，数值单源是 SS.pen 的 v5s 帧（`.scratch/pen-set/wiz/wiz-screens.js`）。
 * 比应用内控件大一档（主行动 h44、内容列 440）：装机现场是一屏一次点一个，不是应用里的密集工具栏。
 * 宽度一律给上限不给死值（`max-w`），只有主行动留着 240 的下限——那让「解除安裝」这类长词不被裁。
 * 只有两个壳引本文件，主应用的页面不引（它有自己的控件族）。
 */
import { getCurrentWindow } from "@tauri-apps/api/window";
import { ChevronDown, ChevronUp, FolderOpen, X } from "lucide-react";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";
import { Tip, TIP_TRIGGER } from "./Tip";
import { HOVER_PRESS } from "./HoverFill";
import { Logo } from "./Logo";
import { CheckBox } from "./Field";

/** 内容列：稿子 440 宽，窗口固定 760、两侧各让 60 */
const COL = "w-full max-w-[440px]";

/**
 * 标题栏：左上角不放图标与程序名（一屏版没有"这是哪个程序"要回答的问题——品牌就在正文里），
 * 右侧留给版本·架构，再过去只有关闭。整条透明：底下的场就是它的面。
 */
export function WizardTitleBar({ meta }: { meta?: string }) {
    const t = useT();
    return (
        <header
            data-tauri-drag-region
            className="flex h-10 shrink-0 select-none items-center gap-2.5 px-4"
        >
            <span data-tauri-drag-region className="min-w-0 flex-1" />
            {meta && (
                <span className="shrink-0 font-mono text-[11px] leading-[16px] text-text-3 tabular-nums">
                    {meta}
                </span>
            )}
            <button
                onClick={() => void getCurrentWindow().close()}
                aria-label={t("wizard.close", "关闭")}
                title={t("wizard.close", "关闭")}
                className={cn(
                    TIP_TRIGGER,
                    "flex h-[26px] w-[34px] items-center justify-center rounded-[6px]",
                    "text-text-2 transition-colors hover:bg-redstone-dim hover:text-redstone"
                )}
            >
                <X className="size-[13px]" />
                <Tip label={t("wizard.close", "关闭")} />
            </button>
        </header>
    );
}

/** 品牌块：logo 那两档色块铺满 64，圆角与投影走卡的那一档（稿子里它是一张卡，不是一枚图标） */
export function WizardHero() {
    return (
        <Logo className="size-16 shrink-0 rounded-[14px] shadow-[var(--shadow-card)]" />
    );
}

/** 正文主词：一屏只有一个，26/700 走标题字 */
export function WizardTitle({ children }: { children: React.ReactNode }) {
    return (
        <h1 className="font-heading text-[26px] leading-[32px] font-bold text-text-1">
            {children}
        </h1>
    );
}

/** 居中正文：字号 13 是"这一屏在说什么"，11 是"顺带要知道的"，两者都不该抢主词 */
export function WizardText({
    children,
    size = 13,
    className,
}: {
    children: React.ReactNode;
    size?: 11 | 12 | 13;
    className?: string;
}) {
    return (
        <p
            className={cn(
                "text-center",
                size === 13 && "text-[13px] leading-[20px]",
                size === 12 && "text-[12px] leading-[18px]",
                size === 11 && "text-[11px] leading-[16px]",
                className
            )}
        >
            {children}
        </p>
    );
}

/** 主行动：一屏只有一颗，accent 实心 + 那圈光；危险那档同形、换色 */
export function WizardCta({
    label,
    onClick,
    kind = "primary",
    disabled,
}: {
    label: string;
    onClick: () => void;
    kind?: "primary" | "danger";
    disabled?: boolean;
}) {
    return (
        <button
            onClick={onClick}
            disabled={disabled}
            className={cn(
                "inline-flex h-11 min-w-60 items-center justify-center rounded-[10px] px-8",
                "text-[13px] leading-[20px] font-bold text-accent-ink",
                HOVER_PRESS,
                kind === "danger"
                    ? "bg-redstone shadow-[var(--shadow-btn-danger)] hover:opacity-90"
                    : "bg-accent shadow-[var(--shadow-btn)] hover:opacity-90",
                "disabled:pointer-events-none disabled:opacity-45 disabled:shadow-none"
            )}
        >
            {label}
        </button>
    );
}

/** 次行动：没有面，只有一行字（稿子里它靠 30 的行高和主行动拉开层级，不靠底色） */
export function WizardLink({
    label,
    onClick,
    chevron,
    tone = "text-2",
    disabled,
}: {
    label: string;
    onClick: () => void;
    /** 就地展开/收起路径行那两档；不给就不画箭头 */
    chevron?: "down" | "up";
    tone?: "text-2" | "text-3" | "text-1";
    disabled?: boolean;
}) {
    const Icon = chevron === "up" ? ChevronUp : ChevronDown;
    return (
        <button
            onClick={onClick}
            disabled={disabled}
            className={cn(
                "inline-flex h-[30px] items-center justify-center gap-1.5 rounded-lg px-2.5",
                "text-[12px] leading-[18px] font-medium",
                HOVER_PRESS,
                tone === "text-1" && "text-text-1",
                tone === "text-2" && "text-text-2 hover:bg-surface-2 hover:text-text-1",
                tone === "text-3" && "text-text-3 hover:bg-surface-2 hover:text-text-2",
                "disabled:pointer-events-none disabled:opacity-45"
            )}
        >
            {label}
            {chevron && <Icon className="size-3" />}
        </button>
    );
}

/**
 * 路径行：只读路径 + 「更改」。它是装机现场唯一的"我要改到哪"出口，所以比应用里的输入框高一档。
 * 值段走 mono：路径是要逐段核对的，比例字体会把 `1`/`l` 混成一样。
 */
export function WizardPathRow({
    value,
    button,
    ariaLabel,
    onBrowse,
}: {
    value: string;
    button: string;
    ariaLabel: string;
    onBrowse: () => void;
}) {
    return (
        <div className={cn("flex items-center gap-2", COL)}>
            <div
                aria-label={ariaLabel}
                className={cn(
                    "flex h-11 min-w-0 flex-1 items-center gap-[9px] rounded-[10px] border border-stroke px-3",
                    "bg-[var(--surface-card)]"
                )}
            >
                <FolderOpen className="size-3.5 shrink-0 text-text-3" />
                <span className="truncate font-mono text-[12px] leading-[18px] font-medium text-text-1">
                    {value}
                </span>
            </div>
            <button
                onClick={onBrowse}
                className={cn(
                    "inline-flex h-11 shrink-0 items-center justify-center rounded-[10px] border border-stroke px-3.5",
                    "bg-veil-pill text-[12px] leading-[18px] font-medium text-text-2",
                    HOVER_PRESS,
                    "hover:bg-surface-2 hover:text-text-1"
                )}
            >
                {button}
            </button>
        </div>
    );
}

/** 进度：一条轨 + 底下「在做什么 / 百分之几」。分母是真实字节数，没有假进度 */
export function WizardProgress({
    pct,
    label,
    danger,
}: {
    pct: number;
    label: string;
    danger?: boolean;
}) {
    const shown = Math.round(pct);
    return (
        <div className={cn("flex flex-col gap-2.5", COL)}>
            <div className="h-1.5 overflow-hidden rounded-full bg-rail-track">
                <span
                    className={cn(
                        "block h-full rounded-full transition-[width] duration-300 ease-out",
                        danger ? "bg-redstone" : "bg-accent"
                    )}
                    style={{ width: `${Math.max(0, Math.min(100, pct))}%` }}
                />
            </div>
            <div className="flex items-baseline justify-between gap-3">
                <span aria-live="polite" className="text-[11px] leading-[16px] text-text-3">
                    {label}
                </span>
                <span className="font-mono text-[11px] leading-[16px] font-semibold text-text-2 tabular-nums">
                    {shown}%
                </span>
            </div>
        </div>
    );
}

/**
 * 「产物留在哪儿」那一句：稿子里路径比前后的字亮一档，所以句子拆成三段渲染。
 * 空格留在译文里（各语言挨着路径的空格不一样），`whitespace-pre` 才不会被 HTML 折叠掉。
 */
export function WizardPathLine({
    before,
    path,
    after,
    danger,
}: {
    before: string;
    path: string;
    after?: string;
    danger?: boolean;
}) {
    const dim = danger ? "text-redstone" : "text-text-3";
    return (
        <p className="flex flex-wrap items-baseline justify-center text-center text-[11px] leading-[16px]">
            <span className={cn("whitespace-pre", dim)}>{before}</span>
            <span
                className={cn(
                    "whitespace-pre break-all font-mono font-medium",
                    danger ? "text-redstone" : "text-text-2"
                )}
            >
                {path}
            </span>
            {after && <span className={cn("whitespace-pre", dim)}>{after}</span>}
        </p>
    );
}

/** 复选行：口径同应用内的设置行——只有方框可点，标签是读数不是第二个按钮 */
export function WizardCheck({
    checked,
    onChange,
    label,
}: {
    checked: boolean;
    onChange: (v: boolean) => void;
    label: string;
}) {
    return (
        <div className="flex items-center gap-2">
            <CheckBox checked={checked} onChange={onChange} />
            <span className="text-[12px] leading-[18px] text-text-2">{label}</span>
        </div>
    );
}
