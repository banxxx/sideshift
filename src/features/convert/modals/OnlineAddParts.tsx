/* ================= 网络添加弹窗的复用件（分段器 / 头像 / 骨架屏 / 筛选下拉） ================= */
import { Check, ChevronDown, Puzzle, Search } from "lucide-react";
import { useEffect, useId, useRef, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import { SEG_PILL_SPRING } from "@/components/ui";
import { cn } from "@/lib/utils";

export type Source = "modrinth" | "curseforge";

/** 筛选下拉的一项：chip = 触发钮上的短文案，label = 列表项全文 */
export interface FilterOpt {
    value: string;
    chip: string;
    label: string;
    group?: string;
}

/** 下载次数 → "1,204 万" / "8,412" */
export function formatCount(n: number): string {
    return n >= 10_000 ? `${(n / 10_000).toFixed(0)} 万` : n.toLocaleString();
}

/** 下载源分段：176×30 轨道 p2 $surface-2 r8；内项 86×26 r6（选中 $accent + 11/600 $accent-ink） */
export function SourceSeg({ value, onChange }: { value: Source; onChange: (s: Source) => void }) {
    const pillId = useId();
    const items: Array<{ key: Source; label: string }> = [
        { key: "modrinth", label: "Modrinth" },
        { key: "curseforge", label: "CurseForge" },
    ];
    return (
        <div className="flex h-[30px] w-[176px] shrink-0 gap-0.5 rounded-lg bg-surface-2 p-0.5">
            {items.map((it) => {
                const active = it.key === value;
                return (
                    <button
                        key={it.key}
                        onClick={() => onChange(it.key)}
                        className={cn(
                            // 宽度用 flex-1 均分：固定 86px 会超出轨道净宽（176-4-2）挤压圆角
                            "relative flex h-[26px] min-w-0 flex-1 items-center justify-center rounded-md text-[11px] leading-[16px] transition-colors",
                            active
                                ? "font-semibold text-accent-ink"
                                : "font-medium text-text-3 hover:text-text-2"
                        )}
                    >
                        {active && (
                            <motion.span
                                layoutId={`${pillId}-src-pill`}
                                transition={SEG_PILL_SPRING}
                                className="absolute inset-0 rounded-md bg-accent"
                            />
                        )}
                        <span className="relative z-[1]">{it.label}</span>
                    </button>
                );
            })}
        </div>
    );
}

/** 模组头像：真实 iconUrl 直链；无图/加载失败回退拼图占位 */
export function ModIcon({
    url,
    className,
    puzzleClass = "size-4",
}: {
    url?: string;
    className?: string;
    puzzleClass?: string;
}) {
    const [failed, setFailed] = useState(false);
    useEffect(() => setFailed(false), [url]);
    const box = cn(
        "flex shrink-0 items-center justify-center overflow-hidden rounded-lg bg-surface-2",
        className ?? "size-9"
    );
    if (!url || failed) {
        return (
            <span className={box}>
                <Puzzle className={cn(puzzleClass, "text-text-2")} />
            </span>
        );
    }
    return (
        <img
            src={url}
            alt=""
            loading="lazy"
            onError={() => setFailed(true)}
            className={cn(box, "object-cover")}
        />
    );
}

/** 列表骨架屏：请求未回来时占住行高，避免布局跳动 */
export function ListSkeleton({ rows = 6, icon = false }: { rows?: number; icon?: boolean }) {
    return (
        <div className="flex min-h-0 flex-1 flex-col gap-0.5">
            {Array.from({ length: rows }, (_, i) => (
                <div key={i} className="flex h-[46px] shrink-0 animate-pulse items-center gap-3 rounded-lg px-2">
                    {icon && <span className="size-9 shrink-0 rounded-lg bg-surface-2" />}
                    <span className="flex min-w-0 flex-1 flex-col gap-1.5">
                        <span className="h-3 w-2/5 rounded bg-surface-2" />
                        <span className="h-2.5 w-3/5 rounded bg-surface-2" />
                    </span>
                </div>
            ))}
        </div>
    );
}

/** 筛选下拉 chip：外观同 FilterChip（h28 r6 + chevron），点开浮层单选；searchable 供长列表（版本）过滤，不自动聚焦 */
export function FilterSelect({
    prefix,
    value,
    options,
    onChange,
    searchable,
    searchPlaceholder = "搜索…",
}: {
    prefix: string;
    value: string;
    options: FilterOpt[];
    onChange: (v: string) => void;
    searchable?: boolean;
    searchPlaceholder?: string;
}) {
    const [open, setOpen] = useState(false);
    const [q, setQ] = useState("");
    const ref = useRef<HTMLDivElement>(null);

    useEffect(() => {
        if (!open) return;
        const onDown = (e: MouseEvent) => {
            if (!ref.current?.contains(e.target as Node)) setOpen(false);
        };
        window.addEventListener("mousedown", onDown);
        return () => window.removeEventListener("mousedown", onDown);
    }, [open]);

    const current = options.find((o) => o.value === value);
    const filtered = options.filter((o) => !q || o.label.toLowerCase().includes(q.toLowerCase()));

    return (
        <div ref={ref} className="relative shrink-0">
            <button
                onClick={() => setOpen((v) => !v)}
                className={cn(
                    "inline-flex h-7 shrink-0 items-center gap-1 rounded-md border bg-surface px-2.5 text-[11px] leading-[16px] font-medium text-text-1 transition-colors hover:bg-surface-2",
                    open ? "border-accent" : "border-stroke"
                )}
            >
                {prefix} {current?.chip ?? value}
                <ChevronDown
                    className={cn("size-3 text-text-2 transition-transform", open && "rotate-180")}
                />
            </button>
            <AnimatePresence>
                {open && (
                    <motion.div
                        initial={{ opacity: 0, y: -6, scaleY: 0.9 }}
                        animate={{ opacity: 1, y: 0, scaleY: 1 }}
                        exit={{ opacity: 0, y: -6, scaleY: 0.9 }}
                        transition={{ duration: 0.16, ease: [0.16, 1, 0.3, 1] }}
                        className="absolute top-full right-0 z-30 mt-1 flex max-h-[248px] min-w-[168px] origin-top flex-col gap-0.5 overflow-hidden rounded-lg border border-stroke bg-surface p-1.5 shadow-lg"
                    >
                        {searchable && (
                            <>
                                <div className="flex h-[26px] shrink-0 items-center gap-1.5 rounded-md px-2">
                                    <Search className="size-3 shrink-0 text-text-3" />
                                    <input
                                        value={q}
                                        onChange={(e) => setQ(e.target.value)}
                                        placeholder={searchPlaceholder}
                                        className="min-w-0 flex-1 bg-transparent text-[11px] text-text-1 outline-none placeholder:text-text-3"
                                    />
                                </div>
                                <div className="h-px w-full shrink-0 bg-stroke" />
                            </>
                        )}
                        <div className="flex min-h-0 flex-col gap-0.5 overflow-auto">
                            {filtered.map((o, i) => {
                                const active = o.value === value;
                                const showGroup = !!o.group && filtered[i - 1]?.group !== o.group;
                                return (
                                    <div key={o.value || "any"} className="flex flex-col">
                                        {showGroup && (
                                            <span className="px-2 pt-1 pb-0.5 text-[10px] leading-[14px] font-normal text-text-3">
                                                {o.group}
                                            </span>
                                        )}
                                        <button
                                            onClick={() => {
                                                onChange(o.value);
                                                setOpen(false);
                                                setQ("");
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
                                    无匹配项
                                </span>
                            )}
                        </div>
                    </motion.div>
                )}
            </AnimatePresence>
        </div>
    );
}
