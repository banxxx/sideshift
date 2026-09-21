/**
 * Convert 页弹窗族（SS.pen `PCRJi` 剔除清单 / `St8m8` 网络添加 / `wphzw` 模组详情）
 *
 * 定稿规则：
 *  - 遮罩 50% 黑；模态 $surface + $stroke 1px r12 padding20 gap12；页脚 = 1px $stroke + 摘要 + 按钮组
 *  - 网络添加与模组详情是同壳二级视图，尺寸强制一致 800×464（“跳转后弹窗不能变小”）
 *  - 处置清单壳剔除/保留/新增共用（focus 区分）：勾选位 = 是否进服务端包，
 *    与卡内 CheckBox 同方向（剔除窗默认全不勾、保留/新增窗默认全勾）；
 *    金色行 = 相对本清单处置被改判、还没应用；勾选只进弹窗草稿，
 *    「应用」时才把差异行回写页面（「取消」/关闭按钮放弃草稿）
 */
import { Check, ChevronDown, ChevronLeft, ChevronRight, Folder, MinusSquare, Puzzle, Search, SquareCheck } from "lucide-react";
import { useEffect, useId, useMemo, useRef, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import * as api from "@/lib/api";
import {
    clientInstallNeeded,
    evidenceLabel,
    formatSize,
    loaderLabel,
    reviewFirst,
    sideTagLabel,
    sideTagOf,
    type SideTag,
} from "@/lib/format";
import type {
    LoaderKind,
    ModDisposition,
    ModSearchResult,
    ModVersionEntry,
    PackDirNode,
    PlanMod,
    VersionOption,
} from "@/lib/types";
import {
    Btn,
    CheckBox,
    ListRow,
    ModalShell,
    SEG_PILL_SPRING,
    SearchBox,
    SegTabs,
    TagChip,
    ToneChip,
} from "@/components/design/ui";
import { cn } from "@/lib/utils";

/* ================= 处置清单弹窗（640 宽，PCRJi 剔除态；保留态共用同壳） ================= */

export type ListFocus = ModDisposition;

/**
 * 三清单共壳（focus 区分）。勾选位统一 = **「这一项进不进服务端包」**，与卡内每行同方向：
 * 剔除窗默认全不勾（勾上 = 改判保留），保留/新增窗默认全勾（取消 = 改判剔除 / 停用）。
 * 金色告警态只跟随「被改判了、还没应用」，不跟随勾选位——否则剔除窗一进来就是满屏黄。
 */
const LIST_COPY: Record<
    ListFocus,
    {
        title: string;
        sub: string;
        /** (维持原处置, 已改判) */
        note: (on: number, off: number) => string;
        /** 改判态行尾的徽章 */
        offBadge: string;
        /** 改判态行的说明文字 */
        rowOff: string;
    }
> = {
    remove: {
        title: "剔除清单",
        sub: "不进服务端包的模组（含判不出两端的待确认项）· 可逐项改回保留",
        note: (on, off) => `${on} 项将剔除 · ${off} 项改为保留`,
        offBadge: "待恢复",
        rowOff: "勾选后改为保留 · 服务端将不剔除",
    },
    keep: {
        title: "保留清单",
        sub: "将随服务端包构建的模组 · 可逐项改回剔除",
        note: (on, off) => `${on} 项将保留 · ${off} 项改为剔除`,
        offBadge: "待剔除",
        rowOff: "取消勾选改为剔除 · 不再进入服务端包",
    },
    add: {
        title: "新增清单",
        sub: "本次转换新增的模组 · 右侧标注端归属，取消勾选即停用，行保留可随时勾回",
        note: (on, off) => `${on} 项将新增 · ${off} 项停用`,
        offBadge: "已停用",
        rowOff: "停用中 · 不进入服务端包，勾选即恢复",
    },
};

/**
 * 行尾芯片 = 筛选档位（同一个 `rowTagOf` 判出来，筛出来的一类必然就是行上看到的那枚标签）：
 * 自动补齐 > 需人工确认 > 端标签。原先这里的计数文字（「已标记 N · 改为保留 M」）与页脚摘要重复，
 * 位置腾给筛选更有用。
 */
type RowTag = SideTag | "autoSupplement";

/** Tab 从左到右的固定顺序；当前清单里计数为 0 的档位不出 Tab */
const ROW_TAG_ORDER: RowTag[] = [
    "autoSupplement",
    "serverRequired",
    "serverOptional",
    "clientRequired",
    "clientOptional",
    "review",
];

function rowTagOf(m: PlanMod, focus: ListFocus): RowTag {
    if (focus === "add" && m.autoSupplement) return "autoSupplement";
    if (m.needsReview) return "review";
    return sideTagOf(m);
}

function rowTagLabel(tag: RowTag): string {
    return tag === "autoSupplement" ? "自动补齐" : sideTagLabel(tag);
}

/** 行尾那枚芯片：优先级只在 `rowTagOf` 一处定义，调用处别再各判一遍 */
function RowTagChip({ m, focus }: { m: PlanMod; focus: ListFocus }) {
    const tag = rowTagOf(m, focus);
    if (tag === "autoSupplement") return <TagChip square>自动补齐</TagChip>;
    if (tag === "review")
        return (
            <TagChip square tone="gold">
                需人工确认
            </TagChip>
        );
    return <SideChip sides={m} square warnClient={focus === "add"} />;
}

/**
 * 端标签芯片（三张清单 + 卡内新增行共用）：服务端必装 / 服务端可选 / 客户端必装 / 客户端可选 / 需人工确认。
 * 判据与 detector 同轴，所以保留组的标签只说服务端、剔除组的标签只说客户端，不会与页签打架。
 * warnClient = 新增清单里的客户端行标红（误下载最典型的一种）
 */
export function SideChip({
    sides,
    square,
    warnClient,
}: {
    sides: Pick<PlanMod, "clientSide" | "serverSide">;
    square?: boolean;
    warnClient?: boolean;
}) {
    const tag = sideTagOf(sides);
    const clientSide = tag === "clientRequired" || tag === "clientOptional";
    return (
        <TagChip
            square={square}
            tone={
                tag === "review" ? "gold" : warnClient && clientSide ? "redstone" : undefined
            }
        >
            {sideTagLabel(tag)}
        </TagChip>
    );
}

/** 剔除行的原因：按实际两侧支持度说，并标出证据出处（不写死「env=client」） */
function stripReason(m: PlanMod): string {
    // 判不出两端：它不是「客户端专属」，是等人来定——措辞必须留出「勾回保留」这条路
    if ((m.envSource ?? "unknown") === "unknown" && m.needsReview) {
        return (
            "无法判定 · 请人工确认：服务端需要就勾回保留" +
            (m.bytecodeHint === "clientOnlyShape" ? " · 字节码形状像纯客户端" : "")
        );
    }
    const why =
        m.serverSide === "unsupported"
            ? "服务端不支持"
            : m.serverSide === "optional"
              ? m.clientSide === "required"
                    ? "客户端必需、服务端仅可选"
                    : "服务端仅可选"
              : m.clientSide === "required"
                ? "客户端必需、服务端没声明"
                : "服务端没声明支持";
    return (
        `剔除原因：${why} · 依据：${evidenceLabel(m.envSource)}` +
        `${m.envConflict ? " · 与整合包声明不一致" : ""}` +
        `${m.needsReview ? " · 待人工确认" : ""}`
    );
}

/** 保留行的原因：没有端证据时必须说「无依据」，不能替模组宣称服务端可用 */
function keepReason(m: PlanMod): string {
    if (m.autoSupplement) return "自动补齐的服务端依赖 · 剔除可能导致启动失败";
    const hint =
        m.bytecodeHint === "serverCode"
            ? " · jar 内确有服务端注册"
            : m.bytecodeHint === "clientOnlyShape"
              ? " · 字节码形状像纯客户端"
              : "";
    const why =
        m.serverSide === "required"
            ? "服务端必需"
            : m.serverSide === "optional"
              ? "服务端可选"
              : m.serverSide === "unsupported"
                ? "服务端不支持，本行未自动剔除"
                : null;
    // 两端必需 = 光进服务端包不算装完，长句说清「还要通知玩家」这件事
    const both = clientInstallNeeded(m) ? " · 玩家客户端需同装" : "";
    return why
        ? `保留原因：${why} · 依据：${evidenceLabel(m.envSource)}${m.envConflict ? " · 与整合包声明不一致" : ""}${both}`
        : `无端证据 · 未自动判定，本行由你保留在包里${hint}`;
}

/** 处于该清单处置下的行说明（一律由证据推导，无证据就承认无证据） */
function rowOnSub(m: PlanMod, focus: ListFocus): string {
    if (focus === "remove") return stripReason(m);
    if (focus === "add") {
        const base = m.autoSupplement
            ? "自动补齐的服务端基础库 · 停用可能导致依赖它的模组失效"
            : m.localPath
              ? "本地 jar · 构建时直接复制"
              : "在线添加 · 已钉住所选构建";
        // 误下载最常见的就是这条：把「服务端不需要」写在行上，而不是等人自己猜
        const tag = sideTagOf(m);
        if (tag === "clientRequired" || tag === "clientOptional") {
            return `${base} · 判为客户端模组，服务端包通常不需要`;
        }
        // 两端都必需 = 装进服务端包还不够，玩家客户端也得装同一个
        if (clientInstallNeeded(m)) {
            return `${base} · 两端必需，玩家客户端需同装`;
        }
        return base;
    }
    return keepReason(m);
}

export function PlanListModal({
    open,
    onClose,
    focus,
    mods,
    onDisposition,
}: {
    open: boolean;
    onClose: () => void;
    /** 清单视角：决定标题/文案与「改判基准」的处置（勾选位恒定 = 进不进服务端包） */
    focus: ListFocus;
    /** 该清单的全部候选（视角过滤由调用方做好后传入） */
    mods: PlanMod[];
    /** 「应用」时回写：只对最终处置与原值不同的行调用 */
    onDisposition: (id: string, d: ModDisposition) => void;
}) {
    const copy = LIST_COPY[focus];
    const [query, setQuery] = useState("");
    /** 行标签筛选档位（与行尾芯片同源），all = 不按标签筛 */
    const [tagFilter, setTagFilter] = useState<RowTag | "all">("all");
    /** 弹窗内暂存：勾选只改 draft，「应用」才回写页面 */
    const [draft, setDraft] = useState<Partial<Record<string, ModDisposition>>>({});

    // 每次打开重建暂存、搜索与筛选（上次未应用的草稿不带入）
    useEffect(() => {
        if (open) {
            setDraft({});
            setQuery("");
            setTagFilter("all");
        }
    }, [open]);

    /* 筛选 Tab：整张清单里真实存在（计数 > 0）的标签才出档位，顺序固定；
       只看「有没有这一类」，数量不上 Tab（标题与页脚已经把数说完了），打字时档位也不跳 */
    const tagItems = useMemo(() => {
        const present = new Set<RowTag>(mods.map((m) => rowTagOf(m, focus)));
        return [
            { key: "all" as RowTag | "all", label: "全部" },
            ...ROW_TAG_ORDER.filter((t) => present.has(t)).map((t) => ({
                key: t,
                label: rowTagLabel(t),
            })),
        ];
    }, [mods, focus]);

    // 待人工确认的行永远在最前（与模组方案卡同一口径）；搜索与标签是 AND
    const filtered = useMemo(
        () =>
            reviewFirst(
                mods.filter(
                    (m) =>
                        (!query || m.name.toLowerCase().includes(query.toLowerCase())) &&
                        (tagFilter === "all" || rowTagOf(m, focus) === tagFilter)
                )
            ),
        [mods, query, tagFilter, focus]
    );

    /** 行当前处置草稿 = 这一块的唯一真相源，勾选/金色/计数/批量全由它派生。
     *  停用行未编辑时的基线视为「关」（add 视角 = 停用即 remove 态） */
    const dispOf = (m: PlanMod): ModDisposition =>
        draft[m.id] ?? (focus === "add" && m.disabled ? "remove" : m.disposition);

    /** 勾选 = 进服务端包（与卡内 CheckBox 同方向，三张清单一致）：
     *  剔除清单默认全不勾、保留/新增清单默认全勾。 */
    const checkedOf = (m: PlanMod): boolean => dispOf(m) !== "remove";
    /** 金色告警 = 相对本清单处置被改判了、还没应用（只跟改判走，不跟勾选走） */
    const pendingOf = (m: PlanMod): boolean => dispOf(m) !== focus;
    /** 页脚「维持原处置」数：剔除=将剔除、保留=将保留、新增=生效 */
    const onN = mods.filter((m) => dispOf(m) === focus).length;

    /** 取消「不进包」时回到的处置：剔除清单里回到保留，保留/新增清单里回到原处置
     *  （新增清单必须回到 add，落成 keep 会被页脚的「生效↔停用」判定当成没改过） */
    const restoreOf = (m: PlanMod): ModDisposition =>
        m.disposition === "remove" ? "keep" : m.disposition;

    const setRow = (m: PlanMod) =>
        setDraft((d) => ({
            ...d,
            [m.id]: dispOf(m) === "remove" ? restoreOf(m) : "remove",
        }));

    /** 批量按钮：目标写死成处置而不是翻转勾选位，作用域 = 搜索 + 标签筛后的可见行。
     *  新增清单的「常态」是生效，所以再点一次的回落位不能等于 focus，得显式停用 */
    const batchTarget: ModDisposition =
        focus === "remove" ? "keep" : focus === "keep" ? "remove" : "add";
    const batchUndo: ModDisposition = focus === "add" ? "remove" : focus;
    const batchDone = filtered.length > 0 && filtered.every((m) => dispOf(m) === batchTarget);
    const batchLabel =
        focus === "remove"
            ? batchDone
                ? "全部维持剔除"
                : "全部勾回保留"
            : focus === "keep"
              ? batchDone
                  ? "全部恢复保留"
                  : "全部改判剔除"
              : batchDone
                ? "全部停用"
                : "全部生效";
    const runBatch = () =>
        setDraft((d) => {
            const next = { ...d };
            filtered.forEach((m) => (next[m.id] = batchDone ? batchUndo : batchTarget));
            return next;
        });

    const apply = () => {
        mods.forEach((m) => {
            const fin = dispOf(m);
            if (focus === "add") {
                // add 视角只对「生效↔停用」真实翻转的行回写（停用不是处置变更）
                const nowOn = fin === "add";
                if (nowOn === !!m.disabled) onDisposition(m.id, nowOn ? "add" : "remove");
            } else if (fin !== m.disposition) {
                onDisposition(m.id, fin);
            }
        });
        onClose();
    };

    return (
        <ModalShell
            open={open}
            onClose={onClose}
            persistent
            width={640}
            height={480}
            title={`${copy.title} · ${mods.length} 个模组`}
            sub={copy.sub}
            footerNote={copy.note(onN, mods.length - onN)}
            footerActions={
                <>
                    <Btn size="sm" className="px-3.5" onClick={onClose}>
                        取消
                    </Btn>
                    <Btn variant="primary" size="sm" className="px-3.5 font-semibold" onClick={apply}>
                        应用
                    </Btn>
                </>
            }
        >
            <SearchBox
                value={query}
                onChange={setQuery}
                placeholder="搜索模组名称…"
                className="border border-stroke"
            />

            {/* toolbar：左批量动作（作用域=当前可见行） + 右标签筛选（计数为 0 的档位不出 Tab） */}
            <div className="flex w-full shrink-0 items-center justify-between gap-2">
                <button
                    className="flex shrink-0 items-center gap-2 text-[11px] leading-[16px] font-medium text-text-2 transition-colors hover:text-text-1"
                    onClick={runBatch}
                >
                    {batchDone ? (
                        <SquareCheck className="size-3.5 text-accent" />
                    ) : (
                        <MinusSquare className="size-3.5 text-accent" />
                    )}
                    {batchLabel}
                </button>
                {/* 极端组合（六档全有 + 长计数）兜一层横向滚动，不把 Tab 挤成换行 */}
                <div className="min-w-0 overflow-x-auto">
                    <SegTabs
                        size="sm"
                        items={tagItems}
                        value={tagFilter}
                        onChange={setTagFilter}
                    />
                </div>
            </div>

            {/* list：gap2；行 padding[8,4]；全部展示（滚动） */}
            <div className="-mx-1 flex min-h-0 flex-1 flex-col gap-0.5 overflow-auto px-1">
                {filtered.map((m) => {
                    const checked = checkedOf(m);
                    const pending = pendingOf(m);
                    return (
                        <ListRow
                            key={m.id}
                            className={cn("cursor-pointer", pending && "bg-gold-dim")}
                            onClick={() => setRow(m)}
                        >
                            {/* 勾选框不接 onChange：点击冒泡到整行，避免一行两处状态源；
                                颜色不跟勾选走（剔除清单的默认未勾选是常态，不该报金） */}
                            <CheckBox checked={checked} onChange={() => {}} />
                            <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                                <span className="truncate font-mono text-[12px] leading-[18px] font-medium text-text-1">
                                    {m.name} {m.version}
                                </span>
                                <span
                                    className={cn(
                                        "truncate text-[10px] leading-[14px] font-normal",
                                        pending ? "text-gold" : "text-text-3"
                                    )}
                                >
                                    {pending ? copy.rowOff : rowOnSub(m, focus)}
                                </span>
                            </span>
                            {pending ? (
                                <TagChip square outline className="text-gold">
                                    {copy.offBadge}
                                </TagChip>
                            ) : (
                                <RowTagChip m={m} focus={focus} />
                            )}
                        </ListRow>
                    );
                })}
                {filtered.length === 0 && (
                    <span className="py-8 text-center text-[11px] text-text-3">无匹配模组</span>
                )}
            </div>
        </ModalShell>
    );
}

/* ================= 目录勾选弹窗（客户端保留目录卡「添加目录」） ================= */

/** 树内目录节点总数（标题「共 N 个目录」口径） */
function countDirNodes(nodes: PackDirNode[]): number {
    return nodes.reduce((s, n) => s + 1 + countDirNodes(n.children), 0);
}

/** 父子去重：祖先已勾选的目录是冗余项（整个父目录都会保留） */
function pruneRedundant(paths: string[]): string[] {
    const sorted = [...paths].sort();
    return sorted.filter((p) => !sorted.some((q) => q !== p && p.startsWith(`${q}/`)));
}

/**
 * 层级目录浏览器：双击进入子目录（含子目录的行），单击勾选（延迟判定避让双击）；
 * 头部返回键 + 面包屑回退层级。勾选先进 draft，「应用」父子去重后整体回写。
 */
export function DirPickerModal({
    open,
    onClose,
    dirs,
    selected,
    onApply,
}: {
    open: boolean;
    onClose: () => void;
    dirs: PackDirNode[];
    selected: string[];
    onApply: (next: string[]) => void;
}) {
    const [query, setQuery] = useState("");
    /** 当前浏览层级（从包根起算的目录段，[]=根） */
    const [path, setPath] = useState<string[]>([]);
    const [draft, setDraft] = useState<string[]>(selected);
    /** 单击延迟句柄：等待第二次点击判定是否双击，避免双击=勾选两次 */
    const clickTimer = useRef<number | null>(null);

    // 每次打开都从当前已选重建暂存与浏览位置（卡片行内移除后再开不会带旧草稿）
    useEffect(() => {
        if (open) {
            setDraft(selected.slice());
            setPath([]);
            setQuery("");
        }
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [open]);

    const levelNodes = useMemo(() => {
        let nodes = dirs;
        for (const seg of path) nodes = nodes.find((n) => n.name === seg)?.children ?? [];
        return nodes;
    }, [dirs, path]);

    const filtered = useMemo(
        () =>
            levelNodes.filter(
                (n) => !query || n.name.toLowerCase().includes(query.toLowerCase())
            ),
        [levelNodes, query]
    );

    /** 行/勾选的唯一键 = 包根起算的相对路径 */
    const keyOf = (n: PackDirNode) => [...path, n.name].join("/");

    const toggle = (key: string) =>
        setDraft((v) => (v.includes(key) ? v.filter((x) => x !== key) : [...v, key]));

    const clearTimer = () => {
        if (clickTimer.current !== null) {
            clearTimeout(clickTimer.current);
            clickTimer.current = null;
        }
    };

    const handleRowClick = (n: PackDirNode) => {
        // 第二次 click 交给 dblclick 处理，这里只撤销挂起的单击
        if (clickTimer.current !== null) {
            clearTimer();
            return;
        }
        const key = keyOf(n);
        if (n.children.length === 0) {
            toggle(key);
            return;
        }
        clickTimer.current = window.setTimeout(() => {
            clickTimer.current = null;
            toggle(key);
        }, 240);
    };

    const handleRowDblClick = (n: PackDirNode) => {
        clearTimer();
        if (n.children.length > 0) setPath((p) => [...p, n.name]);
    };

    // 全选/全取消（切换式）作用于当前层「可见」（含搜索过滤）的目录
    const levelKeys = filtered.map(keyOf);
    const allOn = levelKeys.length > 0 && levelKeys.every((k) => draft.includes(k));
    const toggleAll = () =>
        setDraft((v) =>
            allOn
                ? v.filter((k) => !levelKeys.includes(k))
                : [...new Set([...v, ...levelKeys])]
        );

    return (
        <ModalShell
            open={open}
            onClose={onClose}
            persistent
            back={path.length > 0 ? () => setPath((p) => p.slice(0, -1)) : undefined}
            width={560}
            height={440}
            title={`选择保留目录 · ${countDirNodes(dirs)} 个目录`}
            sub="双击进入子目录 · 单击勾选 · 勾选后随包复制到服务端"
            footerNote={`${draft.length} 个目录将随包保留`}
            footerActions={
                <>
                    <Btn size="sm" className="px-3.5" onClick={onClose}>
                        取消
                    </Btn>
                    <Btn
                        variant="primary"
                        size="sm"
                        className="px-3.5 font-semibold"
                        onClick={() => {
                            clearTimer();
                            onApply(pruneRedundant(draft));
                            onClose();
                        }}
                    >
                        应用
                    </Btn>
                </>
            }
        >
            {/* 面包屑：全部 › kubejs › …（末段为当前位置，其余可点回退） */}
            <div className="flex w-full shrink-0 flex-wrap items-center gap-1 font-mono text-[11px] leading-[16px] text-text-3">
                <button
                    className={cn(
                        "transition-colors",
                        path.length === 0
                            ? "font-medium text-text-1"
                            : "hover:text-text-1"
                    )}
                    onClick={() => setPath([])}
                >
                    全部
                </button>
                {path.map((seg, i) => (
                    <span key={`${seg}-${i}`} className="flex items-center gap-1">
                        <ChevronRight className="size-3" />
                        <button
                            className={cn(
                                "transition-colors",
                                i === path.length - 1
                                    ? "font-medium text-text-1"
                                    : "hover:text-text-1"
                            )}
                            onClick={() => setPath((p) => p.slice(0, i + 1))}
                        >
                            {seg}
                        </button>
                    </span>
                ))}
            </div>

            <SearchBox
                value={query}
                onChange={setQuery}
                placeholder="搜索当前层目录名称…"
                className="border border-stroke"
            />

            {/* toolbar：左全选（切换式，含清空语义） + 右计数 */}
            <div className="flex w-full shrink-0 items-center justify-between gap-2">
                <button
                    className="flex items-center gap-2 text-[11px] leading-[16px] font-medium text-text-2 transition-colors hover:text-text-1"
                    onClick={toggleAll}
                >
                    {allOn ? (
                        <MinusSquare className="size-3.5 text-accent" />
                    ) : (
                        <SquareCheck className="size-3.5 text-accent" />
                    )}
                    {allOn ? "取消全选" : "全选"}
                </button>
                <span className="font-mono text-[11px] leading-[16px] font-normal tabular-nums text-text-3">
                    已勾选 {draft.length} / {countDirNodes(dirs)}
                </span>
            </div>

            <div className="-mx-1 flex min-h-0 flex-1 flex-col gap-0.5 overflow-auto px-1">
                {filtered.map((n) => {
                    const key = keyOf(n);
                    const on = draft.includes(key);
                    return (
                        <ListRow
                            key={key}
                            className="cursor-pointer hover:bg-surface-2"
                            title={n.children.length > 0 ? "双击进入子目录" : undefined}
                            onClick={() => handleRowClick(n)}
                            onDoubleClick={() => handleRowDblClick(n)}
                        >
                            {/* 勾选框即时勾选，不参与双击判定 */}
                            <span onClick={(e) => e.stopPropagation()}>
                                <CheckBox checked={on} onChange={() => toggle(key)} />
                            </span>
                            <Folder
                                className={cn(
                                    "size-4 shrink-0",
                                    on ? "text-accent" : "text-text-3"
                                )}
                            />
                            <span className="min-w-0 flex-1 truncate font-mono text-[12px] leading-[18px] font-medium text-text-1">
                                {n.name}/
                            </span>
                            <span
                                className={cn(
                                    "shrink-0 font-mono text-[11px] leading-[16px] tabular-nums",
                                    on ? "text-emerald" : "text-text-3"
                                )}
                            >
                                {n.fileCount} 文件
                            </span>
                        </ListRow>
                    );
                })}
                {filtered.length === 0 && (
                    <span className="py-8 text-center text-[11px] text-text-3">
                        {dirs.length === 0 ? "包内没有可保留的目录" : "无匹配目录"}
                    </span>
                )}
            </div>
        </ModalShell>
    );
}

/* ================= 网络添加弹窗（800×464，St8m8 + 详情 wphzw） ================= */

type Source = "modrinth" | "curseforge";

/** 筛选下拉的一项：chip = 触发钮上的短文案，label = 列表项全文 */
interface FilterOpt {
    value: string;
    chip: string;
    label: string;
    group?: string;
}

export function OnlineAddModal({
    open,
    onClose,
    mcVersion,
    loader,
    onAdd,
}: {
    open: boolean;
    onClose: () => void;
    mcVersion: string;
    loader: LoaderKind;
    /** 选中某个构建版本后回写新增列表 */
    onAdd: (mod: ModSearchResult, version: ModVersionEntry) => void;
}) {
    const [view, setView] = useState<"list" | "detail">("list");
    const [source, setSource] = useState<Source>("modrinth");
    const [query, setQuery] = useState("");
    const [debounced, setDebounced] = useState("");
    const [page, setPage] = useState(1);
    // 三筛选值："" = 全部版本；"" = 任意加载器；"all" = 全部类别
    const [verSel, setVerSel] = useState(mcVersion);
    const [loSel, setLoSel] = useState<LoaderKind | "">(loader);
    const [catSel, setCatSel] = useState("all");
    const [mcOptions, setMcOptions] = useState<VersionOption[]>([]);
    const [categories, setCategories] = useState<string[]>([]);
    const [detail, setDetail] = useState<ModSearchResult | null>(null);
    const [versions, setVersions] = useState<ModVersionEntry[]>([]);
    const [versionsLoading, setVersionsLoading] = useState(false);
    const [versionsError, setVersionsError] = useState<string | null>(null);
    const [result, setResult] = useState<{ total: number; results: ModSearchResult[] }>({
        total: 0,
        results: [],
    });
    const [loading, setLoading] = useState(false);
    const [error, setError] = useState<string | null>(null);

    // 弹窗壳常驻挂载：每次打开重置回一级视图与包自身版本/加载器
    useEffect(() => {
        if (!open) return;
        setView("list");
        setDetail(null);
        setQuery("");
        setDebounced("");
        setPage(1);
        setVerSel(mcVersion);
        setLoSel(loader);
        setCatSel("all");
    }, [open, mcVersion, loader]);

    // 搜索词防抖 300ms：真实后端逐键请求会打爆 Modrinth
    useEffect(() => {
        const t = setTimeout(() => setDebounced(query), 300);
        return () => clearTimeout(t);
    }, [query]);

    // 筛选下拉的真实选项：MC 版本表 + Modrinth 官方类别标签
    useEffect(() => {
        if (!open) return;
        void api.listMcVersions().then(setMcOptions).catch(() => {});
        void api.listModCategories().then(setCategories).catch(() => {});
    }, [open]);

    useEffect(() => {
        if (!open) return;
        let alive = true;
        setLoading(true);
        setError(null);
        void api
            .searchMods({
                source,
                text: debounced,
                mcVersion: verSel,
                loader: loSel || null,
                category: catSel,
                page,
            })
            .then((p) => {
                if (!alive) return;
                setResult({ total: p.total, results: p.results });
                setLoading(false);
            })
            .catch((e: unknown) => {
                if (!alive) return;
                setResult({ total: 0, results: [] });
                setError(e instanceof Error ? e.message : String(e));
                setLoading(false);
            });
        return () => {
            alive = false;
        };
    }, [open, source, debounced, page, verSel, loSel, catSel]);

    const openDetail = (mod: ModSearchResult) => {
        setDetail(mod);
        setView("detail");
        setVersions([]);
        setVersionsError(null);
        setVersionsLoading(true);
        void api
            .listModVersions(mod.id)
            .then(setVersions)
            .catch((e: unknown) => setVersionsError(e instanceof Error ? e.message : String(e)))
            .finally(() => setVersionsLoading(false));
    };

    const close = () => {
        onClose();
        setView("list");
        setDetail(null);
    };

    const verOpts = useMemo<FilterOpt[]>(
        () => [
            { value: "", chip: "全部", label: "全部版本" },
            ...mcOptions.map((o) => ({
                value: o.value,
                chip: o.value,
                label: o.value,
                group: o.group,
            })),
        ],
        [mcOptions]
    );
    const loOpts = useMemo<FilterOpt[]>(
        () => [
            { value: "", chip: "任意", label: "任意加载器" },
            { value: "fabric", chip: "Fabric", label: "Fabric" },
            { value: "forge", chip: "Forge", label: "Forge" },
            { value: "neoforge", chip: "NeoForge", label: "NeoForge" },
        ],
        []
    );
    const catOpts = useMemo<FilterOpt[]>(
        () => [
            { value: "all", chip: "全部", label: "全部类别" },
            ...categories.map((c) => ({ value: c, chip: c, label: c })),
        ],
        [categories]
    );

    // 二级视图共用同一筛选状态：版本行按所选版本/加载器在前端过滤
    const shownVersions = useMemo(
        () =>
            versions.filter(
                (v) => (!verSel || v.mcVersion === verSel) && (!loSel || v.loader === loSel)
            ),
        [versions, verSel, loSel]
    );

    const filterNote = `${verSel ? `Minecraft ${verSel}` : "全部版本"} · ${loSel ? loaderLabel(loSel) : "任意加载器"}`;

    /* 模组名后那一枚端标签（全弹窗只此一处，列表行与版本行都不再挂）：
       优先项目级支持度，平台只在构建级给数据时退到首个有声明的构建；两边都没有就不挂 */
    const modTag = useMemo(() => {
        if (!detail) return null;
        if (detail.clientSide || detail.serverSide) return detail;
        return shownVersions.find((v) => v.clientSide || v.serverSide) ?? null;
    }, [detail, shownVersions]);

    /* ---- 二级视图：模组详情 + 版本列表（整行点击下载） ---- */
    if (view === "detail" && detail) {
        return (
            <ModalShell
                open={open}
                onClose={close}
                persistent
                back={() => setView("list")}
                width={800}
                height={464}
                icon={Puzzle}
                iconNode={<ModIcon url={detail.iconUrl} className="size-10" puzzleClass="size-5" />}
                title={detail.name}
                titleTag={modTag ? <SideChip sides={modTag} warnClient /> : undefined}
                sub={`${source === "modrinth" ? "Modrinth" : "CurseForge"} · 作者 ${detail.author} · ${formatCount(detail.downloads)} 次下载`}
            >
                <p className="shrink-0 text-[13px] leading-[20px] font-normal text-text-2">
                    {detail.description}
                </p>
                <div className="flex h-[30px] w-full shrink-0 items-center gap-2">
                    <div className="flex h-7 items-center gap-2">
                        <FilterSelect
                            prefix="版本"
                            value={verSel}
                            options={verOpts}
                            searchable
                            onChange={setVerSel}
                        />
                        <FilterSelect
                            prefix="加载器"
                            value={loSel}
                            options={loOpts}
                            onChange={(v) => setLoSel(v as LoaderKind | "")}
                        />
                    </div>
                </div>
                <div className="-mx-1 flex min-h-0 flex-1 flex-col gap-0.5 overflow-auto px-1">
                    {versionsLoading ? (
                        <ListSkeleton rows={5} />
                    ) : versionsError ? (
                        <span className="py-8 text-center text-[11px] text-gold">
                            版本加载失败 · {versionsError}
                        </span>
                    ) : (
                        <>
                            {shownVersions.map((v, i) => (
                                <ListRow
                                    key={v.id}
                                    className={cn(
                                        "cursor-pointer hover:bg-surface-2",
                                        i === shownVersions.length - 1 && "bg-surface"
                                    )}
                                    onClick={() => {
                                        onAdd(detail, v);
                                        close();
                                    }}
                                >
                                    <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                                        <span className="flex items-center gap-2">
                                            <span className="truncate font-mono text-[12px] leading-[18px] font-medium text-text-1">
                                                {v.versionNumber}
                                            </span>
                                            {v.recommended && (
                                                <ToneChip tone="gold" size="xs">
                                                    推荐
                                                </ToneChip>
                                            )}
                                        </span>
                                        <span className="truncate text-[10px] leading-[14px] font-normal text-text-3">
                                            Minecraft {v.mcVersion} · {loaderLabel(v.loader)} ·{" "}
                                            {v.date} · {formatSize(v.sizeBytes)}
                                        </span>
                                    </span>
                                </ListRow>
                            ))}
                            {shownVersions.length === 0 && (
                                <span className="py-8 text-center text-[11px] text-text-3">
                                    {versions.length === 0
                                        ? "该模组没有可用构建"
                                        : "当前筛选下没有构建"}
                                </span>
                            )}
                        </>
                    )}
                </div>
            </ModalShell>
        );
    }

    /* ---- 一级视图：搜索结果列表 ---- */
    return (
        <ModalShell
            open={open}
            onClose={close}
            persistent
            width={800}
            height={464}
            title="从网络添加模组"
            sub={`搜索 Modrinth 与 CurseForge · ${filterNote}`}
            footerNote={
                error
                    ? `${source === "modrinth" ? "Modrinth" : "CurseForge"} · 加载失败`
                    : `${source === "modrinth" ? "Modrinth" : "CurseForge"} · 共 ${result.total} 个结果`
            }
            footerActions={
                <>
                    <Btn
                        size="sm"
                        icon={ChevronLeft}
                        className="w-9 px-0"
                        disabled={page <= 1 || loading}
                        onClick={() => setPage((p) => p - 1)}
                        title="上一页"
                    />
                    <Btn
                        size="sm"
                        icon={ChevronRight}
                        className="w-9 bg-surface px-0"
                        disabled={loading || page * result.results.length >= result.total}
                        onClick={() => setPage((p) => p + 1)}
                        title="下一页"
                    />
                    <Btn variant="primary" size="sm" className="px-3.5 font-semibold" onClick={close}>
                        完成
                    </Btn>
                </>
            }
        >
            <SearchBox
                value={query}
                onChange={(v) => {
                    setQuery(v);
                    setPage(1);
                }}
                placeholder="搜索模组名称…"
                className="bg-surface-2"
            />

            {/* toolbar：下载源分段（176×30）+ 真实筛选下拉组（h28） */}
            <div className="flex h-[30px] w-full shrink-0 items-center justify-between gap-2">
                <SourceSeg
                    value={source}
                    onChange={(s) => {
                        setSource(s);
                        setPage(1);
                    }}
                />
                <div className="flex h-7 items-center gap-2">
                    <FilterSelect
                        prefix="版本"
                        value={verSel}
                        options={verOpts}
                        searchable
                        onChange={(v) => {
                            setVerSel(v);
                            setPage(1);
                        }}
                    />
                    <FilterSelect
                        prefix="加载器"
                        value={loSel}
                        options={loOpts}
                        onChange={(v) => {
                            setLoSel(v as LoaderKind | "");
                            setPage(1);
                        }}
                    />
                    <FilterSelect
                        prefix="类别"
                        value={catSel}
                        options={catOpts}
                        onChange={(v) => {
                            setCatSel(v);
                            setPage(1);
                        }}
                    />
                </div>
            </div>

            {/* 结果行：真实图标 + 整行点入模组详情；请求中显示骨架屏 */}
            <div className="-mx-1 flex min-h-0 flex-1 flex-col gap-0.5 overflow-auto px-1">
                {loading ? (
                    <ListSkeleton rows={6} icon />
                ) : error ? (
                    <span className="py-8 text-center text-[11px] text-gold">
                        加载失败 · {error}
                    </span>
                ) : (
                    <>
                        {result.results.map((m, i) => (
                            <ListRow
                                key={m.id}
                                className={cn(
                                    "cursor-pointer hover:bg-surface-2",
                                    i === result.results.length - 1 && "bg-surface"
                                )}
                                onClick={() => openDetail(m)}
                            >
                                <ModIcon url={m.iconUrl} />
                                <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                                    <span className="truncate font-mono text-[12px] leading-[18px] font-medium text-text-1">
                                        {m.name}
                                    </span>
                                    <span className="truncate text-[10px] leading-[14px] font-normal text-text-3">
                                        {m.description}
                                    </span>
                                </span>
                                {m.alreadyAdded && (
                                    <ToneChip tone="emerald" size="xs">
                                        已添加
                                    </ToneChip>
                                )}
                                <ChevronRight className="size-3.5 shrink-0 text-text-3" />
                            </ListRow>
                        ))}
                        {result.results.length === 0 && (
                            <span className="py-8 text-center text-[11px] text-text-3">
                                无匹配结果
                            </span>
                        )}
                    </>
                )}
            </div>
        </ModalShell>
    );
}

/** 下载源分段：176×30 轨道 p2 $surface-2 r8；内项 86×26 r6（选中 $accent + 11/600 $accent-ink） */
function SourceSeg({ value, onChange }: { value: Source; onChange: (s: Source) => void }) {
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
function ModIcon({
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
function ListSkeleton({ rows = 6, icon = false }: { rows?: number; icon?: boolean }) {
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
function FilterSelect({
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

/** 下载次数 → "1,204 万" / "8,412" */
function formatCount(n: number): string {
    return n >= 10_000 ? `${(n / 10_000).toFixed(0)} 万` : n.toLocaleString();
}
