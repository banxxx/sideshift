/**
 * 下拉选择框 —— 全应用只有这一个下拉（浮层、动效、搜索行、选中态都只有一份实现）
 *
 * 三种皮肤，共用同一套浮层：
 *  - field（默认）：整宽字段，label 在上。触发框 h32 padding[0,10] 两端对齐 $surface + $stroke 1px r8，
 *    值 等宽 12/500 $text-1；展开态描边 $accent 1.5px（设计稿定稿）。
 *  - plain：Settings 行内态，$bg-app 底、值 等宽 11 $text-2；展开只把描边换成 $accent，
 *    不加粗——行内跟着加粗会让整行文字抖 0.5px。
 *  - chip：弹窗工具栏的紧凑筛选钮 h28 r6，「前缀 + 短文案」（短文案取 option.chip），浮层右对齐。
 *    密度与 field 不同是刻意的：工具栏只有 30px 高，塞不进 h32 字段。
 *
 * 开合统一走同一组动效（向下抽出 + 位移 + 淡入淡出）+ chevron 旋转 180°，
 * 三条收尾路径（选中、点外部、再点一次）都过 close()，不留「有的下拉没动画」的缝隙。
 *
 * searchable 的口径：**选项写死的就不给搜索框**（Java 档位、游戏模式、难度、下载源、加载器
 * 统共几项，搜索框只会让浮层白高一截）；服务端来的长列表才给（MC 版本、加载器版本、
 * 模组版本、类别）。搜索框一律不自动聚焦：点开是为了先看列表，抢焦点会让滚轮落进输入框。
 */
import { Check, ChevronDown, Search } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import { cn } from "@/lib/utils";

export interface SelectOption {
    value: string;
    label: string;
    recommended?: boolean;
    group?: string;
    /** chip 皮肤触发钮上的短文案（缺省回退 label）：筛选项因此能显示「全部」而不是「全部版本」 */
    chip?: string;
}

/** 浮层动效：以顶部为原点纵向抽出，配轻微位移与淡入淡出 */
const PANEL_MOTION = {
    initial: { opacity: 0, y: -6, scaleY: 0.9 },
    animate: { opacity: 1, y: 0, scaleY: 1 },
    exit: { opacity: 0, y: -6, scaleY: 0.9 },
    // ease 得留成定长元组： widened 成 number[] 后 motion 认不出是三次贝塞尔
    transition: { duration: 0.16, ease: [0.16, 1, 0.3, 1] as [number, number, number, number] },
};

export function SearchSelect({
    label,
    prefix,
    value,
    options,
    onChange,
    variant = "field",
    plain,
    searchable,
    searchPlaceholder = "搜索…",
    className,
}: {
    label?: string;
    /** chip 皮肤的前缀（"版本"/"加载器"/"类别"），field 皮肤用 label 走上方 */
    prefix?: string;
    value: string;
    options: SelectOption[];
    onChange: (v: string) => void;
    variant?: "field" | "chip";
    plain?: boolean;
    searchable?: boolean;
    searchPlaceholder?: string;
    className?: string;
}) {
    const [open, setOpen] = useState(false);
    const [query, setQuery] = useState("");
    const boxRef = useRef<HTMLDivElement>(null);
    const isChip = variant === "chip";

    // 点击外部关闭：查询词一并清掉，下次点开是完整列表
    useEffect(() => {
        if (!open) return;
        const onDown = (e: MouseEvent) => {
            if (!boxRef.current?.contains(e.target as Node)) {
                setOpen(false);
                setQuery("");
            }
        };
        window.addEventListener("mousedown", onDown);
        return () => window.removeEventListener("mousedown", onDown);
    }, [open]);

    const close = () => {
        setOpen(false);
        setQuery("");
    };
    const q = query.toLowerCase();
    const filtered =
        searchable && q
            ? options.filter(
                  (o) =>
                      o.label.toLowerCase().includes(q) ||
                      o.value.toLowerCase().includes(q) ||
                      (o.chip ?? "").toLowerCase().includes(q)
              )
            : options;
    const current = options.find((o) => o.value === value);

    return (
        <div
            ref={boxRef}
            className={cn("relative flex flex-col gap-1.5", isChip ? "shrink-0" : "min-w-0", className)}
        >
            {label && (
                <span className="text-[11px] leading-[16px] font-normal text-text-3">{label}</span>
            )}
            <button
                onClick={() => (open ? close() : setOpen(true))}
                className={cn(
                    "flex items-center justify-between gap-2 border transition-colors",
                    isChip
                        ? "h-7 shrink-0 self-start rounded-md bg-surface px-2 text-[11px] leading-[16px] font-medium text-text-1 hover:bg-surface-2"
                        : "h-8 w-full rounded-lg px-2.5",
                    !isChip && (plain ? "bg-bg-app" : "bg-surface"),
                    open
                        ? plain || isChip
                            ? "border-accent"
                            : "border-[1.5px] border-accent"
                        : "border-stroke"
                )}
            >
                <span
                    className={cn(
                        "truncate",
                        isChip
                            ? "text-[11px] leading-[16px] font-medium"
                            : plain
                              ? "font-mono text-[11px] leading-[16px] font-normal text-text-2"
                              : "font-mono text-[12px] leading-[18px] font-medium text-text-1"
                    )}
                >
                    {prefix ? `${prefix} ` : ""}
                    {current?.chip ?? current?.label ?? value}
                </span>
                <ChevronDown
                    className={cn(
                        "size-3 shrink-0 transition-transform duration-200",
                        open ? "rotate-180 text-accent" : "text-text-3"
                    )}
                />
            </button>
            <AnimatePresence>
                {open && (
                    <motion.div
                        {...PANEL_MOTION}
                        className={cn(
                            "absolute top-full z-30 mt-1 flex max-h-[248px] w-max max-w-[320px] origin-top flex-col gap-0.5",
                            "overflow-hidden rounded-lg border border-stroke bg-surface p-1.5 shadow-lg",
                            // 浮层按最长选项撑开（w-max，封顶 320px），不跟触发框等宽：
                            // 行选中态还要留 20px 给 check，等宽会把长标签掐成省略号。
                            // 锚边按触发件位置定：行右端的 chip/plain 向左长，卡内整列的 field 向右长，
                            // 反了会把滚动祖先顶出一道横向滚动条
                            isChip ? "right-0 min-w-[168px]" : plain ? "right-0 min-w-full" : "left-0 min-w-full"
                        )}
                    >
                        {searchable && (
                            <>
                                <div className="flex h-[26px] shrink-0 items-center gap-1.5 rounded-md px-2">
                                    <Search className="size-3 shrink-0 text-text-3" />
                                    <input
                                        value={query}
                                        onChange={(e) => setQuery(e.target.value)}
                                        placeholder={searchPlaceholder}
                                        // size=1 + w-0：input 的固有宽度约 20 字符，
                                        // 浮层是 w-max，不掐掉它搜索框会把面板顶到 320px 上限
                                        size={1}
                                        className="w-0 min-w-0 flex-1 bg-transparent text-[11px] text-text-1 outline-none placeholder:text-text-3"
                                    />
                                </div>
                                <div className="h-px w-full shrink-0 bg-stroke" />
                            </>
                        )}
                        <div className="flex min-h-0 flex-col gap-0.5 overflow-auto">
                            {filtered.map((o, i) => {
                                const active = o.value === value;
                                // 分组标题只在该组第一行出现（同组连续排列），不再逐行重复
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
                                                close();
                                            }}
                                            className={cn(
                                                "flex h-[26px] w-full items-center justify-between gap-2 rounded-md px-2 transition-colors",
                                                active ? "bg-surface-2" : "hover:bg-surface-2"
                                            )}
                                        >
                                            <span
                                                className={cn(
                                                    "min-w-0 flex-1 truncate font-mono text-[12px] leading-[18px]",
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
