/**
 * 可搜索下拉
 *
 * 触发框（Convert 卡内）：h32 padding[0,10] 两端对齐 $surface + $stroke 1px r8；
 *   值 等宽 12/500 $text-1，chevron-down 12 $text-3；展开态描边换成 $accent 1.5px + chevron-up $accent
 * 触发框（plain，Settings 行内）：$bg-app 底，值 等宽 11 $text-2
 * 面板：padding 6 gap2 $surface + $stroke 1px r8；搜索行 h26；分组标 10 $text-3；
 *   选项 h26 padding[0,8] r6，选中 $surface-2 + 12/600 $accent + check 12
 */
import { Check, ChevronDown, ChevronUp, Search } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import { cn } from "@/lib/utils";

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
