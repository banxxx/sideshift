/**
 * 表单控件：步进器 / 开关 / 输入框 / 勾选框 / 搜索行
 *
 * 字号阶梯 10/11/12/13/14/16/22；小元素 r6，控件 r8。
 */
import { Check, Minus, Plus, Search, X, type LucideIcon } from "lucide-react";
import { cn } from "@/lib/utils";
import { Tip, TIP_TRIGGER } from "./Tip";

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
    readOnly,
}: {
    value: number;
    onChange: (v: number) => void;
    min?: number;
    max?: number;
    suffix?: string;
    plain?: boolean;
    /** 回看态：只留值区，± 与分隔线一起去掉（留着就是「能点却不能点」的假出口） */
    readOnly?: boolean;
}) {
    const step = (d: number) => onChange(Math.min(max, Math.max(min, value + d)));
    const valueCls = cn(
        "inline-flex h-8 items-center justify-center font-mono text-[12px] leading-[18px] font-semibold text-text-1",
        plain ? "w-10" : "px-3.5"
    );
    if (readOnly)
        return (
            <div
                className={cn(
                    "flex h-8 shrink-0 items-center rounded-lg border border-stroke",
                    !plain && "bg-surface"
                )}
            >
                <span className={valueCls}>
                    {value}
                    {suffix && <span className="ml-1">{suffix}</span>}
                </span>
            </div>
        );
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
            <span className={valueCls}>
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
    readOnly,
}: {
    checked: boolean;
    onChange: (v: boolean) => void;
    size?: "sm" | "md";
    disabled?: boolean;
    /** 回看态：不跟 `disabled` 共用那条灰化规则——开与关都是要核对的信息，压暗就看不出来了 */
    readOnly?: boolean;
}) {
    return (
        <button
            role="switch"
            aria-checked={checked}
            aria-disabled={readOnly}
            disabled={disabled}
            onClick={readOnly ? undefined : () => onChange(!checked)}
            className={cn(
                "flex shrink-0 items-center rounded-full p-0.5 transition-colors",
                "disabled:pointer-events-none disabled:opacity-60",
                readOnly && "pointer-events-none",
                size === "sm" ? "h-5 w-9" : "h-[22px] w-[38px] p-[3px]",
                checked ? "justify-end bg-accent" : "justify-start border border-stroke bg-surface-2"
            )}
        >
            <span className="size-4 shrink-0 rounded-full bg-white" />
        </button>
    );
}

/* ---------------- 输入框：h32 r8；卡内 $surface-2 无描边，plain 为 $bg-app + $stroke ---------------- */

export function TextInput({
    icon: Icon,
    plain,
    className,
    readOnly,
    ...rest
}: React.ComponentProps<"input"> & { icon?: LucideIcon; plain?: boolean }) {
    return (
        <div
            className={cn(
                "flex h-8 shrink-0 items-center gap-[7px] rounded-lg px-2.5",
                plain ? "border border-stroke bg-bg-app" : "bg-surface-2",
                // 回看态不配焦点环：亮起来是在说「这里能打字」，而它只是段可选中的值
                !readOnly && "focus-within:ring-1 focus-within:ring-accent",
                className
            )}
        >
            {Icon && <Icon className="size-3 shrink-0 text-text-3" />}
            <input
                readOnly={readOnly}
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

/* ---------------- 勾选框：16×16 r4（Convert 模组行 + 处置清单弹窗行） ----------------
 * 选中 $accent + check 10 $accent-ink；未选 $surface + $stroke；待确认未选 $surface + $gold
 */
export function CheckBox({
    checked,
    review,
    onChange,
    readOnly,
}: {
    checked: boolean;
    review?: boolean;
    onChange: (v: boolean) => void;
    /** 回看态：勾选态照原样显示（它是「这一项进没进包」的读数），但不接点击、不留悬停 */
    readOnly?: boolean;
}) {
    return (
        <button
            role="checkbox"
            aria-checked={checked}
            aria-disabled={readOnly}
            onClick={readOnly ? undefined : () => onChange(!checked)}
            className={cn(
                "flex size-4 shrink-0 items-center justify-center rounded transition-colors",
                checked
                    ? "bg-accent"
                    : review
                      ? "border border-gold bg-surface"
                      : cn("border border-stroke bg-surface", !readOnly && "hover:border-text-3")
            )}
        >
            {checked && <Check className="size-2.5 text-accent-ink" strokeWidth={3} />}
        </button>
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
                    aria-label="清空搜索"
                    onClick={() => onChange("")}
                    className={cn(
                        TIP_TRIGGER,
                        "size-4 rounded text-text-3 transition-colors hover:bg-surface-2 hover:text-text-1",
                        "flex shrink-0 items-center justify-center"
                    )}
                >
                    <X className="size-3" />
                    <Tip label="清空搜索" />
                </button>
            )}
        </div>
    );
}
