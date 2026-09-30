/**
 * 下拉选择框 —— 全应用只有这一个下拉（浮层、动效、搜索行、选中态都只有一份实现）。
 * 三种皮肤共用同一套浮层：field（整宽字段，展开描边 $accent 1.5px）/ plain（行内态，展开只换描边不加粗，防整行文字抖 0.5px）/ chip（工具栏紧凑 h28，密度不同是刻意的）。
 * 开合统一走同一组动效 + chevron 转 180°；三条收尾路径（选中、点外部、再点一次）都过 close()。
 * searchable 口径：选项写死的短列表不给搜索框，服务端来的长列表才给；搜索框一律不自动聚焦。
 */
import { Check, ChevronDown, Search } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import { cn } from "@/lib/utils";
import { tSource, useT } from "@/lib/i18n";
import { READONLY_BOX } from "./Field";
import { HOVER_FILL } from "./HoverFill";

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
    searchPlaceholder,
    placeholder,
    panelFit,
    className,
    readOnly,
    onOpen,
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
    /** 值为空时的那格外显（裸 zip 认不出 MC 版本那一态）：不给就照旧留白，别处用法不受影响 */
    placeholder?: string;
    /**
     * 浮层与触发框等宽、长标签走省略号（默认是按最长选项撑开，封顶 320px）。
     * 给「触发框本来就占满整栏，而候选文字长度由用户自己打」的那一处用：转换页配置模板的下拉住在
     * 280px 右栏里，模板名最长 40 字，让它撑宽既顶穿窗口边、又和下面那张摘要卡对不齐——等宽 + 省略才稳。
     * 别处不吃这一档：版本/构建号那一长串被等宽掐成省略号就等于没得选。
     */
    panelFit?: boolean;
    className?: string;
    /** 回看态：值照显示，chevron 与浮层一起收掉——留着箭头就是「能点却不能点」的假出口 */
    readOnly?: boolean;
    /**
     * 点开浮层那一刻回调一次。给候选会过期的那一档用（Java 下拉的候选是本机实探出来的，
     * 用户装完一枚 JDK 回到页面时列表不会自己变）：点开就是"我要看现在有什么"，
     * 这一趟重探花几十毫秒，省掉的是拿一份假列表让人挑。
     */
    onOpen?: () => void;
}) {
    const t = useT();
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
    /** 空值 + 给了外显文案 ⇒ 这一格显示提示而不是留白（值不在列表里仍原样显示，那是另一回事） */
    const showingPlaceholder = !value && !current && !!placeholder;

    return (
        <div
            ref={boxRef}
            className={cn(
                "relative flex flex-col gap-1.5",
                isChip ? "shrink-0" : "min-w-0",
                // 禁光标挂外层：disabled 的 button 上浏览器不一定绘出自定义光标，
                // 而外层 div 一直在命中区里，指针一改就全程是「不可点」
                readOnly && "cursor-not-allowed",
                className
            )}
        >
            {label && (
                <span className="text-[11px] leading-[16px] font-normal text-text-3">{label}</span>
            )}
            <button
                disabled={readOnly}
                onClick={() => {
                    if (open) close();
                    else {
                        onOpen?.();
                        setOpen(true);
                    }
                }}
                className={cn(
                    "flex items-center justify-between gap-2 border",
                    HOVER_FILL,
                    isChip
                        ? "h-7 shrink-0 self-start rounded-md bg-surface px-2 text-[11px] leading-[16px] font-medium text-text-1 hover:bg-surface-2"
                        : "h-8 w-full rounded-lg px-2.5",
                    !isChip && (plain ? "bg-bg-app" : "bg-surface"),
                    open
                        ? plain || isChip
                            ? "border-accent"
                            : "border-[1.5px] border-accent"
                        : "border-stroke",
                    // 回看态：与输入框同一套「凹下去 + 描边降一档」；chip 是设置页的可用件，不参与
                    !isChip && readOnly && READONLY_BOX
                )}
            >
                <span
                    className={cn(
                        // min-w-0 是 truncate 生效的前提：flex 子项默认 min-width:auto，
                        // 长名（模板名最长 40 字）会把触发框里的值顶到 chevron 外面去，省略号根本不出现
                        "min-w-0 truncate",
                        isChip
                            ? "text-[11px] leading-[16px] font-medium"
                            : plain
                              ? "font-mono text-[11px] leading-[16px] font-normal text-text-2"
                              : "font-mono text-[12px] leading-[18px] font-medium text-text-1",
                        !isChip && readOnly && "text-text-2",
                        // 空值那格用正文字体 + 三级色：等宽体是给版本号准备的，汉字摆在里面既不像值也不够弱
                        showingPlaceholder && "font-sans text-[12px] font-normal text-text-3"
                    )}
                >
                    {prefix ? `${prefix} ` : ""}
                    {showingPlaceholder
                        ? placeholder
                        : tSource(current?.chip ?? current?.label ?? value)}
                </span>
                {!readOnly && (
                    <ChevronDown
                        className={cn(
                            "size-3 shrink-0 transition-transform duration-200",
                            open ? "rotate-180 text-accent" : "text-text-3"
                        )}
                    />
                )}
            </button>
            <AnimatePresence>
                {open && (
                    <motion.div
                        {...PANEL_MOTION}
                        className={cn(
                            "absolute top-full z-30 mt-1 flex max-h-[248px] origin-top flex-col gap-0.5",
                            // 宽度两档：默认按最长选项撑开（封顶 320px），`panelFit` 则与触发框等宽
                            panelFit ? "w-full" : "w-max max-w-[320px]",
                            "overflow-hidden rounded-lg border border-stroke bg-surface p-1.5 shadow-lg",
                            // 浮层默认不跟触发框等宽（行选中态还要留 20px 给 check，等宽会把长标签掐成省略号），
                            // 所以长列表那一档靠 w-max 撑开；只有用户自己打名的候选（模板）走 panelFit。
                            // 锚边按触发件位置定：行右端的 chip/plain 向左长，卡内整列的 field 向右长，
                            // 反了会把滚动祖先顶出一道横向滚动条。
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
                                        placeholder={searchPlaceholder ?? t("common.search", "搜索…")}
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
                                                {/*i18n:正式版*/}
                                                {/*i18n:推荐*/}
                                                {/*i18n:最新*/}
                                                {/*i18n:全部构建*/}
                                                {tSource(o.group!)}
                                            </span>
                                        )}
                                        <button
                                            onClick={() => {
                                                onChange(o.value);
                                                close();
                                            }}
                                            className={cn(
                                                "flex h-[26px] w-full items-center justify-between gap-2 rounded-md px-2",
                                                HOVER_FILL,
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
                                                {/* 选项文案来自后端枚举（下载源、版本通道…）：就地查目录，
                                                    查不到就是原文，所以前端已翻过的选项不受影响 */}
                                                {/*i18n:官方源*/}
                                                {/*i18n:BMCLAPI 国内镜像*/}
                                                {tSource(o.label)}
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
                                    {t("common.matches", "无匹配项")}
                                </span>
                            )}
                        </div>
                    </motion.div>
                )}
            </AnimatePresence>
        </div>
    );
}
