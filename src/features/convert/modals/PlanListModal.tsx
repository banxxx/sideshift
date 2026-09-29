/* ================= 处置清单弹窗（640 宽，PCRJi 剔除态；保留态共用同壳） ================= */
import { Check, Copy, MinusSquare, SquareCheck } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import {
    clientInstallNeeded,
    evidenceLabel,
    reviewFirst,
    sideTagLabel,
    sideTagOf,
    type SideTag,
} from "@/lib/format";
import { t, useT } from "@/lib/i18n";
import { notify } from "@/lib/notify";
import type { ModDisposition, PlanMod } from "@/lib/types";
import {
    Btn,
    CheckBox,
    ListRow,
    ModalShell,
    SearchBox,
    SegTabs,
    Swap,
    TagChip,
    HOVER_FILL,
} from "@/components/ui";
import { cn } from "@/lib/utils";
import { SideChip } from "./SideChip";

export type ListFocus = ModDisposition;

/**
 * 三清单共壳（focus 区分）。勾选位统一 = **「这一项进不进服务端包」**，与卡内每行同方向：
 * 剔除窗默认全不勾（勾上 = 改判保留），保留/新增窗默认全勾（取消 = 改判剔除 / 停用）。
 * 金色告警态只跟随「被改判了、还没应用」，不跟随勾选位——否则剔除窗一进来就是满屏黄。
 */
const listCopy = (): Record<
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
> => ({
    remove: {
        title: t("convert-modals.removed-list", "剔除清单"),
        sub: t("convert-modals.mods-server", "不进服务端包的模组（含判不出两端的待确认项）"),
        editNote: t("convert-modals.re-kept", "可逐项改回保留"),
        note: (on, off) => t("convert-modals.remove-off", "{{on}} 项将剔除 · {{off}} 项改为保留", { on, off }),
        offBadge: t("convert-modals.restore", "待恢复"),
        rowOff: t("convert-modals.check-keep", "勾选后改为保留 · 服务端将不剔除"),
    },
    keep: {
        title: t("convert-modals.kept-list", "保留清单"),
        sub: t("convert-modals.mods-built", "将随服务端包构建的模组"),
        editNote: t("convert-modals.re-removed", "可逐项改回剔除"),
        note: (on, off) => t("convert-modals.keep-off", "{{on}} 项将保留 · {{off}} 项改为剔除", { on, off }),
        offBadge: t("convert-modals.remove", "待剔除"),
        rowOff: t("convert-modals.uncheck-remove", "取消勾选改为剔除 · 不再进入服务端包"),
    },
    add: {
        title: t("convert-modals.added-list", "新增清单"),
        sub: t("convert-modals.mods-added", "本次转换新增的模组 · 右侧标注端归属"),
        editNote: t("convert-modals.uncheck-disable", "取消勾选即停用，行保留可随时勾回"),
        note: (on, off) => t("convert-modals.active-off", "{{on}} 项将新增 · {{off}} 项停用", { on, off }),
        offBadge: t("convert-modals.disabled", "已停用"),
        rowOff: t("convert-modals.disabled-server", "停用中 · 不进入服务端包，勾选即恢复"),
    },
});

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
    return tag === "autoSupplement" ? t("convert.auto-added", "自动补齐") : sideTagLabel(tag);
}

/** 行尾那枚芯片：优先级只在 `rowTagOf` 一处定义，调用处别再各判一遍 */
function RowTagChip({ m, focus }: { m: PlanMod; focus: ListFocus }) {
    const t = useT();
    const tag = rowTagOf(m, focus);
    if (tag === "autoSupplement") return <TagChip square>{t("convert.auto-added", "自动补齐")}</TagChip>;
    if (tag === "review")
        return (
            <TagChip square tone="gold">
                {t("lib.needs-review", "需人工确认")}
            </TagChip>
        );
    return <SideChip sides={m} square warnClient={focus === "add"} />;
}

/** 剔除行的原因：按实际两侧支持度说，并标出证据出处（不写死「env=client」） */
function stripReason(m: PlanMod): string {
    // 判不出两端：它不是「客户端专属」，是等人来定——措辞必须留出「勾回保留」这条路
    if ((m.envSource ?? "unknown") === "unknown" && m.needsReview) {
        return t(
            "convert-modals.undecided-review", "无法判定 · 请人工确认：服务端需要就勾回保留{{hint}}",
            {
                hint:
                    m.bytecodeHint === "clientOnlyShape"
                        ? t("convert-modals.bytecode-looks", " · 字节码形状像纯客户端")
                        : "",
            }
        );
    }
    const why =
        m.serverSide === "unsupported"
            ? t("convert-modals.server-unsupported", "服务端不支持")
            : m.serverSide === "optional"
              ? m.clientSide === "required"
                    ? t("convert-modals.client-required", "客户端必需、服务端仅可选")
                    : t("convert-modals.server-only", "服务端仅可选")
              : m.clientSide === "required"
                ? t("convert-modals.client-required-server", "客户端必需、服务端没声明")
                : t("convert-modals.server-support", "服务端没声明支持");
    return t(
        "convert-modals.remove-reason", "剔除原因：{{why}} · 依据：{{evidence}}{{conflict}}{{review}}",
        {
            why,
            evidence: evidenceLabel(m.envSource),
            conflict: m.envConflict ? t("convert-modals.differs-modpack", " · 与整合包声明不一致") : "",
            review: m.needsReview ? t("convert-modals.needs-review", " · 待人工确认") : "",
        }
    );
}

/** 保留行的原因：没有端证据时必须说「无依据」，不能替模组宣称服务端可用 */
function keepReason(m: PlanMod): string {
    if (m.autoSupplement) return t("convert-modals.auto-added", "自动补齐的服务端依赖 · 剔除可能导致启动失败");
    const hint =
        m.bytecodeHint === "serverCode"
            ? t("convert-modals.jar-server", " · jar 内确有服务端注册")
            : m.bytecodeHint === "clientOnlyShape"
              ? t("convert-modals.bytecode-looks", " · 字节码形状像纯客户端")
              : "";
    const why =
        m.serverSide === "required"
            ? t("convert-modals.server-required", "服务端必需")
            : m.serverSide === "optional"
              ? t("lib.server-optional", "服务端可选")
              : m.serverSide === "unsupported"
                ? t("convert-modals.server-unsupported-auto", "服务端不支持，本行未自动剔除")
                : null;
    // 两端必需 = 光进服务端包不算装完，长句说清「还要通知玩家」这件事
    return why
        ? t(
            "convert-modals.keep-reason", "保留原因：{{why}} · 依据：{{evidence}}{{conflict}}{{both}}",
            {
                why,
                evidence: evidenceLabel(m.envSource),
                conflict: m.envConflict ? t("convert-modals.differs-modpack", " · 与整合包声明不一致") : "",
                both: clientInstallNeeded(m) ? t("convert-modals.players-install", " · 玩家客户端需同装") : "",
            }
        )
        : t("convert-modals.side-evidence", "无端证据 · 未自动判定，本行由你保留在包里{{hint}}", { hint });
}

/** 处于该清单处置下的行说明（一律由证据推导，无证据就承认无证据） */
function rowOnSub(m: PlanMod, focus: ListFocus): string {
    if (focus === "remove") return stripReason(m);
    if (focus === "add") {
        const base = m.autoSupplement
            ? t("convert-modals.auto-added-server", "自动补齐的服务端基础库 · 停用可能导致依赖它的模组失效")
            : m.localPath
              ? t("convert-modals.local-jar", "本地 jar · 构建时直接复制")
              : t("convert-modals.added-online", "在线添加 · 已钉住所选构建");
        // 误下载最常见的就是这条：把「服务端不需要」写在行上，而不是等人自己猜
        const tag = sideTagOf(m);
        if (tag === "clientRequired" || tag === "clientOptional") {
            return t("convert-modals.base-client", "{{base}} · 判为客户端模组，服务端包通常不需要", { base });
        }
        // 两端都必需 = 装进服务端包还不够，玩家客户端也得装同一个
        if (clientInstallNeeded(m)) {
            return t("convert-modals.base-sides", "{{base}} · 两端必需，玩家客户端需同装", { base });
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
    const t = useT();
    const copy = listCopy()[focus];
    const [query, setQuery] = useState("");
    /** 行标签筛选档位（与行尾芯片同源），all = 不按标签筛 */
    const [tagFilter, setTagFilter] = useState<RowTag | "all">("all");
    /** 弹窗内暂存：勾选只改 draft，「应用」才回写页面 */
    const [draft, setDraft] = useState<Partial<Record<string, ModDisposition>>>({});
    /** 刚复制过的那一行（图标换 Check 的绿色回执）；一次只记一行 */
    const [copiedId, setCopiedId] = useState<string | null>(null);
    const copyTimer = useRef(0);

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
            { key: "all" as RowTag | "all", label: t("common.entry-2", "全部") },
            ...ROW_TAG_ORDER.filter((tt) => present.has(tt)).map((tt) => ({
                key: tt,
                label: rowTagLabel(tt),
            })),
        ];
    }, [mods, focus, t]);

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

    /** 复制行名（只名称，不带版本）。反馈就地换图标、不发全局提示（与日志复制同一口径）；
     *  只有剪贴板本身不可用这一种失败值得占提示区 */
    const copyName = async (m: PlanMod) => {
        try {
            await navigator.clipboard.writeText(m.name);
        } catch {
            notify(t("common.copy-failed", "复制失败：剪贴板不可用"), "error");
            return;
        }
        window.clearTimeout(copyTimer.current);
        setCopiedId(m.id);
        copyTimer.current = window.setTimeout(() => setCopiedId(null), 1800);
    };

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
            title={t("convert-modals.title-count", "{{title}} · {{count}} 个模组", { title: copy.title, count: mods.length })}
            sub={
                readOnly
                    ? copy.sub
                    : t("convert-modals.sub-edit-note", "{{sub}} · {{editNote}}", {
                          sub: copy.sub,
                          editNote: copy.editNote,
                      })
            }
            footerNote={readOnly ? undefined : copy.note(onN, mods.length - onN)}
            footerActions={
                <>
                    <Btn size="sm" className="px-3.5" onClick={onClose}>
                        {readOnly ? t("common.close", "关闭") : t("common.cancel", "取消")}
                    </Btn>
                    {!readOnly && (
                        <Btn
                            variant="primary"
                            size="sm"
                            className="px-3.5 font-semibold"
                            onClick={apply}
                        >
                            {t("convert-modals.apply", "应用")}
                        </Btn>
                    )}
                </>
            }
        >
            <SearchBox
                value={query}
                onChange={setQuery}
                placeholder={t("convert-modals.search-mod", "搜索模组名称…")}
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
                        className={`flex shrink-0 items-center gap-2 text-[11px] leading-[16px] font-medium text-text-2 hover:text-text-1 ${HOVER_FILL}`}
                        onClick={runBatch}
                    >
                        {batchDone ? (
                            <SquareCheck className="size-3.5 text-accent" />
                        ) : (
                            <MinusSquare className="size-3.5 text-accent" />
                        )}
                        {/* 名称不随视角定制（「全部勾回保留」这类自造词有歧义）：
                            勾/不勾的语义由行勾选位本身表达，这里只做可见行的全选 */}
                        {batchDone ? t("convert-modals.unselect-2", "取消全部") : t("common.entry-2", "全部")}
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

            {/* list：gap2；行 padding[8,4]；全部展示（滚动）。
                切筛选档 = 同一位置换一批行 ⇒ 走全站那一档 Swap 节拍（进 180ms 落 6px / 出 90ms 溶解）。
                key 只认 tagFilter：**打字筛（query）不演**，那是一行一行地少下去，演一次整块换批反而
                读成「列表被重刷了」。Swap 自带 relative 外壳，所以这层滚动容器不用再加定位。
                list-scroll：内容宽度必须恒定，否则从行少的档切走时底部会闪一条横向滚动条
                （退场层带的是切换前量好的 px 宽度，口径见 App.css 的 .list-scroll）。 */}
            <div className="list-scroll -mx-1 flex min-h-0 flex-1 flex-col overflow-auto px-1">
                <Swap swapKey={tagFilter} className="gap-0.5">
                    {filtered.map((m) => {
                        const checked = checkedOf(m);
                        const pending = pendingOf(m);
                        return (
                            <ListRow
                                key={m.id}
                                className={cn(
                                    // 具名悬停组：无名 group 会和行内其它悬停件串味（见 Tip 的注释）
                                    "group/row",
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
                                {/* 逐行复制：槽位常驻（24 + 行 gap 10），只有图标藏到悬停。
                                    留白是为「正看着的那行文字不跳」，藏图标是为「三十行同屏不是一排灰按钮」。
                                    不挂 Tip：气泡在 .list-scroll 这个 overflow-auto 盒里，贴底那行会被裁掉；
                                    Copy 字形自解释，可读名交给 aria-label */}
                                <button
                                    aria-label={t("convert-modals.copy-name", "复制名称")}
                                    onClick={(e) => {
                                        // 整行是改判的点击靶区：不挡冒泡就成了一点两改（复制 + 把这行踢出服务端包）
                                        e.stopPropagation();
                                        void copyName(m);
                                    }}
                                    className={cn(
                                        "size-6 shrink-0 rounded-md text-text-3",
                                        "flex items-center justify-center",
                                        "opacity-0 transition-[opacity,color,background-color] duration-150",
                                        "hover:bg-surface-2 hover:text-accent",
                                        "focus-visible:opacity-100 group-hover/row:opacity-100"
                                    )}
                                >
                                    {copiedId === m.id ? (
                                        <Check className="size-3 text-emerald" />
                                    ) : (
                                        <Copy className="size-3" />
                                    )}
                                </button>
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
                        <span className="py-8 text-center text-[11px] text-text-3">
                            {t("convert-modals.matching-mods", "无匹配模组")}
                        </span>
                    )}
                </Swap>
            </div>
        </ModalShell>
    );
}
