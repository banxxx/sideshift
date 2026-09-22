/* ================= 处置清单弹窗（640 宽，PCRJi 剔除态；保留态共用同壳） ================= */
import { MinusSquare, SquareCheck } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import {
    clientInstallNeeded,
    evidenceLabel,
    reviewFirst,
    sideTagLabel,
    sideTagOf,
    type SideTag,
} from "@/lib/format";
import type { ModDisposition, PlanMod } from "@/lib/types";
import {
    Btn,
    CheckBox,
    ListRow,
    ModalShell,
    SearchBox,
    SegTabs,
    TagChip,
} from "@/components/ui";
import { cn } from "@/lib/utils";
import { SideChip } from "./SideChip";

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
        /** 事实描述：清单里装的是哪些行，回看态也只说这一句 */
        sub: string;
        /** 可编辑态追加的操作指引（回看态不拼，否则是在承诺一个不存在的能力） */
        editNote: string;
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
        sub: "不进服务端包的模组（含判不出两端的待确认项）",
        editNote: "可逐项改回保留",
        note: (on, off) => `${on} 项将剔除 · ${off} 项改为保留`,
        offBadge: "待恢复",
        rowOff: "勾选后改为保留 · 服务端将不剔除",
    },
    keep: {
        title: "保留清单",
        sub: "将随服务端包构建的模组",
        editNote: "可逐项改回剔除",
        note: (on, off) => `${on} 项将保留 · ${off} 项改为剔除`,
        offBadge: "待剔除",
        rowOff: "取消勾选改为剔除 · 不再进入服务端包",
    },
    add: {
        title: "新增清单",
        sub: "本次转换新增的模组 · 右侧标注端归属",
        editNote: "取消勾选即停用，行保留可随时勾回",
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
    "unknown",
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
    readOnly,
    onDisposition,
}: {
    open: boolean;
    onClose: () => void;
    /** 清单视角：决定标题/文案与「改判基准」的处置（勾选位恒定 = 进不进服务端包） */
    focus: ListFocus;
    /** 该清单的全部候选（视角过滤由调用方做好后传入） */
    mods: PlanMod[];
    /** 回看态：只读浏览——搜索与标签筛选照常（那是看，不是改），批量/勾选/应用一并摘掉 */
    readOnly?: boolean;
    /** 「应用」时回写：只对最终处置与原值不同的行调用；只读视图不传（没有「应用」这枚按钮） */
    onDisposition?: (id: string, d: ModDisposition) => void;
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
                if (nowOn === !!m.disabled) onDisposition?.(m.id, nowOn ? "add" : "remove");
            } else if (fin !== m.disposition) {
                onDisposition?.(m.id, fin);
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
            sub={readOnly ? copy.sub : `${copy.sub} · ${copy.editNote}`}
            footerNote={readOnly ? undefined : copy.note(onN, mods.length - onN)}
            footerActions={
                <>
                    <Btn size="sm" className="px-3.5" onClick={onClose}>
                        {readOnly ? "关闭" : "取消"}
                    </Btn>
                    {!readOnly && (
                        <Btn
                            variant="primary"
                            size="sm"
                            className="px-3.5 font-semibold"
                            onClick={apply}
                        >
                            应用
                        </Btn>
                    )}
                </>
            }
        >
            <SearchBox
                value={query}
                onChange={setQuery}
                placeholder="搜索模组名称…"
                className="border border-stroke"
            />

            {/* toolbar：左批量动作（作用域=当前可见行） + 右标签筛选（计数为 0 的档位不出 Tab）。
                回看态没有批量动作，筛选档位从「靠右」改成整行右对齐，避免左半边空出一截 */}
            <div
                className={cn(
                    "flex w-full shrink-0 items-center gap-2",
                    readOnly ? "justify-end" : "justify-between"
                )}
            >
                {!readOnly && (
                    <button
                        className="flex shrink-0 items-center gap-2 text-[11px] leading-[16px] font-medium text-text-2 transition-colors hover:text-text-1"
                        onClick={runBatch}
                    >
                        {batchDone ? (
                            <SquareCheck className="size-3.5 text-accent" />
                        ) : (
                            <MinusSquare className="size-3.5 text-accent" />
                        )}
                        {/* 名称不随视角定制（「全部勾回保留」这类自造词有歧义）：
                            勾/不勾的语义由行勾选位本身表达，这里只做可见行的全选 */}
                        {batchDone ? "取消全部" : "全部"}
                    </button>
                )}
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
                            className={cn(
                                !readOnly && "cursor-pointer",
                                pending && "bg-gold-dim"
                            )}
                            onClick={readOnly ? undefined : () => setRow(m)}
                        >
                            {/* 勾选框不接 onChange：点击冒泡到整行，避免一行两处状态源；
                                颜色不跟勾选走（剔除清单的默认未勾选是常态，不该报金） */}
                            <CheckBox checked={checked} readOnly={readOnly} onChange={() => {}} />
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
