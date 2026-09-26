/**
 * 模组方案卡：分段 Tab + 预览行 + 依赖警告 + 卡底出口。
 *
 * 卡高固定 260：切 Tab / 行数变化 / 出口块高低差全部由行区（flex-1）吸收，卡片外形不随内容抖。
 * 入场节拍（谁进入可见列表谁拿一档）在本组件内算：它是纯展示节奏，
 * 只依赖当前 Tab 与可见行，放这里比留在页面里更能说明「行是自己进出场的」。
 */
import { AlertTriangle, File, Globe } from "lucide-react";
import { useRef } from "react";
import { AnimatePresence, motion } from "motion/react";
import type { ModDisposition, PlanMod } from "@/lib/types";
import { Btn, Collapse, LinkBtn, Panel, PanelHead, SegTabs } from "@/components/ui";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";
import { PLAN_LAND, PLAN_LAYOUT } from "@/lib/springs";
import { badgeFor, PlanModRow } from "./PlanModRow";
import { LAND_MAX_STEPS, LAND_STAGGER_MS } from "./constants";
import type { ListFocus } from "./modals";

export interface DepWarning {
    missing: PlanMod;
    hosts: PlanMod[];
}

export function ModPlanCard({
    tab,
    onTab,
    counts,
    rows,
    classifying = false,
    totalRows,
    readingLabel = "",
    emptyLabel,
    depWarnings,
    localIds,
    removableIds,
    manualEdits = 0,
    confirmClear = false,
    readOnly,
    onToggleRow,
    onRemoveRow,
    onRestoreDep,
    onOpenList,
    onReclassify,
    onConfirmClear,
    onClearEdits,
    onAddLocal,
    onAddOnline,
}: {
    tab: ModDisposition;
    onTab: (t: ModDisposition) => void;
    /** 页签与出口计数：add = 生效新增数，addTotal = 清单行数（弹窗口径） */
    counts: { remove: number; keep: number; add: number; addTotal: number };
    /** 当前页签的可见预览行（口径由页面算好，节拍也按它算） */
    rows: PlanMod[];
    classifying?: boolean;
    /** 方案是否已有行（分类首屏的空卡要说「正在读取整合包…」而不是「暂无模组」） */
    totalRows: number;
    readingLabel?: string;
    /** 方案一行都没有时的说法（回看态由页面给出：那时「暂无模组」是在说存档，不是在说分类） */
    emptyLabel?: string;
    depWarnings: DepWarning[];
    /** 本地 .jar 添加的模组 id（徽章显示「本地」而非「推荐」） */
    localIds?: Set<string>;
    /** 本页新增行 id（决定 × 是否出现；系统补行只能停用不能抹掉） */
    removableIds?: Set<string>;
    /** 手动改动数（处置覆写 + 停用行）：>0 才露出「清空我的修改」出口 */
    manualEdits?: number;
    confirmClear?: boolean;
    /** 回看态：只给查看清单的出口，不给改判/重跑/添加的出口（行勾选与 × 一并静态化） */
    readOnly?: boolean;
    /** 以下写入口在只读视图（任务详情「方案」签）里一概不传：静态渲染下它们没有触发路径 */
    /** 行的勾选：add 行 = 生效/停用切换，其余 = remove↔keep 改判 */
    onToggleRow?: (m: PlanMod) => void;
    onRemoveRow?: (m: PlanMod) => void;
    /** 依赖警告的「恢复」：停用行 = 重新生效，剔除行 = 改判保留（两种行语义不同，由页面决定） */
    onRestoreDep?: (m: PlanMod) => void;
    onOpenList: (f: ListFocus) => void;
    onReclassify?: () => void;
    onConfirmClear?: (on: boolean) => void;
    onClearEdits?: () => void;
    onAddLocal?: () => void;
    onAddOnline?: () => void;
}) {
    /* 入场节拍：谁「进入可见列表」谁拿一档（0、1、2…按可见顺序自上而下），换页签即重置。
       ——必须在渲染期算，不能放 effect：motion 在挂载那一趟 layout effect 里就启动 initial→animate，
       之后再改 transition.delay 也不会重启动画，effect 补档位已经晚了
       （这就是「重跑分类时五行一起出现」的原因；切页签看着正常只是骗人——外层 mode="wait"
       要等旧页签淡出，新行挂载时 effect 早就补好了档位）。
       用 rows 的引用当幂等闩：同一批数据只算一次，StrictMode 双跑也只算一次。 */
    const t = useT();
    const cadence = useRef({
        rows: null as PlanMod[] | null,
        tab: "" as ModDisposition,
        shown: new Set<string>(),
        slots: new Map<string, number>(),
    }).current;
    if (cadence.tab !== tab) {
        cadence.tab = tab;
        cadence.shown.clear();
        cadence.slots.clear();
    }
    if (cadence.rows !== rows) {
        const visible = new Set(rows.map((m) => m.id));
        // 离开的行既忘掉「已见过」也忘掉旧档位：再回到列表就算新入场
        cadence.shown.forEach((id) => {
            if (!visible.has(id)) cadence.shown.delete(id);
        });
        cadence.slots.forEach((_, id) => {
            if (!visible.has(id)) cadence.slots.delete(id);
        });
        const fresh = rows.filter((m) => !cadence.shown.has(m.id));
        fresh.forEach((m) => cadence.shown.add(m.id));
        // 增量合并而不是整表替换：上一批还在排队的行不能被后来的批次抢走档位
        fresh.forEach((m, i) => cadence.slots.set(m.id, Math.min(i, LAND_MAX_STEPS)));
        cadence.rows = rows;
    }
    const landDelayOf = (id: string) => (cadence.slots.get(id) ?? 0) * LAND_STAGGER_MS;

    return (
        <Panel gap={14} className="h-[260px]">
            <PanelHead
                title={t("convert.mod-plan", "模组方案")}
                right={
                    <SegTabs
                        items={[
                            {
                                key: "remove" as ModDisposition,
                                label: t("convert.removed", "剔除"),
                                count: counts.remove,
                            },
                            { key: "keep" as ModDisposition, label: t("convert.kept", "保留"), count: counts.keep },
                            { key: "add" as ModDisposition, label: t("convert.added", "新增"), count: counts.add },
                        ]}
                        value={tab}
                        onChange={onTab}
                    />
                }
            />

            {/* 行区：行区顶部铺开、出口 mt-auto 钉在卡底（下方只剩 p-5 的 20 内边距）；
                内容高低差产生的余量全部落在「列表 ↔ 出口」之间，切页签/出警告时出口不跳位 */}
            <div className="flex min-h-0 flex-1 flex-col gap-2.5 overflow-hidden">
                <AnimatePresence mode="wait" initial={false}>
                    <motion.div
                        key={tab}
                        /* 容器只做淡入：换页签时的位移全部交给下面的逐行插入，
                           否则「整块上下浮 + 逐行左右插」两支动画会互相抢读感 */
                        initial={{ opacity: 0 }}
                        animate={{ opacity: 1 }}
                        exit={{ opacity: 0 }}
                        transition={{ duration: 0.14, ease: "easeOut" }}
                        className="flex min-h-0 shrink flex-col gap-2.5 overflow-hidden"
                    >
                        {rows.length === 0 ? (
                            /* 分类中不给任何行：离线那一批会在联网轮落地时被成批改写，
                               先显示出来就是先给一遍会被推翻的结论（而且当场可改判）。
                               方案还没读到时说「正在读取…」；已读到但联网没跑完时留白——
                               卡底那句「自动分类中」不在这儿重复一遍，卡高固定 260 不会跳 */
                            classifying ? (
                                totalRows === 0 ? (
                                    <p className="flex h-[140px] w-full items-center justify-center px-6 text-center text-[11px] leading-[16px] text-text-3">
                                        {readingLabel}
                                    </p>
                                ) : null
                            ) : (
                                <p className="flex h-[140px] w-full items-center justify-center px-6 text-center text-[11px] leading-[16px] text-text-3">
                                    {totalRows === 0 && emptyLabel
                                        ? emptyLabel
                                        : t("convert.mods-group", "该分类下暂无模组")}
                                </p>
                            )
                        ) : (
                            /* 逐行进出：新进入可见列表的行从右侧一档一档插进来，
                               改判离开的行往左退场，其余行由 layout 弹簧补位。
                               这里必须允许首帧入场（不写 initial={false}）：
                               切页签时整块内容是新挂载的，一旦禁掉首帧，
                               新页签的五行就只跟着容器淡入、读不出逐行插入 */
                            <AnimatePresence mode="popLayout">
                                {rows.map((m) => {
                                    /* 一档 110ms：本批新进入可见列表的行自上而下依次从右侧插入；
                                       单独一行的档位天然是 0，所以手动勾选仍是立即响应 */
                                    const step = landDelayOf(m.id);
                                    return (
                                        <motion.div
                                            key={m.id}
                                            layout="position"
                                            initial={{ opacity: 0, x: 44 }}
                                            animate={{ opacity: 1, x: 0 }}
                                            exit={{
                                                opacity: 0,
                                                x: -26,
                                                transition: {
                                                    duration: 0.16,
                                                    ease: "easeIn",
                                                },
                                            }}
                                            transition={{
                                                /* layout 不排队：补位要立刻跟上，
                                                   否则后面的行会先僵住再弹走 */
                                                layout: PLAN_LAYOUT,
                                                /* 轻微回弹，落位时「顿」一下 */
                                                default: {
                                                    ...PLAN_LAND,
                                                    delay: step / 1000,
                                                },
                                            }}
                                            className="min-w-0"
                                        >
                                            <PlanModRow
                                                mod={m}
                                                badge={badgeFor(m, localIds?.has(m.id) ?? false)}
                                                readOnly={readOnly}
                                                onToggle={() => onToggleRow?.(m)}
                                                onRemove={
                                                    !readOnly &&
                                                    m.disposition === "add" &&
                                                    removableIds?.has(m.id)
                                                        ? () => onRemoveRow?.(m)
                                                        : undefined
                                                }
                                            />
                                        </motion.div>
                                    );
                                })}
                            </AnimatePresence>
                        )}
                    </motion.div>
                </AnimatePresence>

                {/* 反向依赖警告：保留/生效新增行依赖了被剔除或被停用的模组，一键恢复即消警。
                    这块以前只演了 opacity+y、没演高度 ⇒ 出现/消警时下面那排与卡底出口整块当场跳一格。
                    现在交公共折叠件（父容器是 flex 列 gap-2.5=10）；金底与内边距留在壳内的盒子上，
                    壳必须是空壳，否则收不到 0。 */}
                <Collapse when={depWarnings.length > 0} gap={10}>
                    <div className="flex shrink-0 flex-col gap-1 rounded-lg bg-gold-dim px-3 py-2">
                        {depWarnings.slice(0, 2).map(({ missing, hosts }) => (
                            <div key={missing.id} className="flex items-center gap-2">
                                <AlertTriangle className="size-3.5 shrink-0 text-gold" />
                                <span className="min-w-0 flex-1 truncate text-[11px] leading-[16px] text-gold">
                                    {t("convert.hosts-depend", "{{hosts}} 依赖被{{action}}的 {{name}}", {
                                        // 整句一个键 + 槽：英文的「depend on …」词序和中文同轴，拆成两截就拼不回来
                                        hosts:
                                            hosts
                                                .slice(0, 2)
                                                .map((h) => h.name)
                                                .join("、") +
                                            (hosts.length > 2
                                                ? t("convert.count", " 等 {{count}} 项", { count: hosts.length })
                                                : ""),
                                        action: missing.disabled ? t("convert.disabled", "停用") : t("convert.removed", "剔除"),
                                        name: missing.name,
                                        // 模组名/版本是数据：关掉 i18next 的 HTML 转义，
                                        // 否则 `Alex's Mobs` 这类名字会显示成 `Alex&#39;s Mobs`
                                        interpolation: { escapeValue: false },
                                    })}
                                </span>
                                {!readOnly && (
                                    <LinkBtn size="sm" onClick={() => onRestoreDep?.(missing)}>
                                        {t("convert.restore", "恢复")}
                                    </LinkBtn>
                                )}
                            </div>
                        ))}
                        {depWarnings.length > 2 && (
                            <span className="pl-[22px] text-[10px] leading-[14px] text-gold">
                                {readOnly
                                    ? t("convert.count-conflict", "… 另有 {{count}} 组依赖冲突（回看只记当时结论，不在此处理）", {
                                          count: depWarnings.length - 2,
                                      })
                                    : t("convert.count-conflict-group", "… 另有 {{count}} 组依赖冲突，可逐项恢复处理", {
                                          count: depWarnings.length - 2,
                                      })}
                            </span>
                        )}
                    </div>
                </Collapse>

                {/* 出口区（行区内、钉在卡底）：剔除/保留态取链接自然高（~18），
                    新增态=查看链接居左、两枚 h32 添加按钮居右；上下不虚占固定高 */}
                <div className="mt-auto flex shrink-0 flex-col gap-2">
                    {/* 分类中整卡锁定：出口只给「一句数 + 一条 rail」，
                        查看全部清单 / 重新自动分类 / 添加模组 都要等结论落定才露出 */}
                    {classifying && (
                        <div className="flex w-full items-center gap-2.5">
                            <span className="shrink-0 text-[11px] leading-[16px] text-text-3">
                                {totalRows === 0
                                    ? readingLabel
                                    : t("convert.auto-classifying", "自动分类中 · 完成后一次性给出方案")}
                            </span>
                            {/* 不确定式扫描条（复用 Shift Rail 的 sheen 语言）：分类期间屏上没有任何行，
                                这一条是整卡唯一的进度信号。在线反查按批回结论，按批算百分比会一步跳到
                                100%，所以这里只说「还在跑」，不报数也不报比例 */}
                            <span className="rail-flow h-1.5 min-w-0 flex-1 rounded-full" />
                        </div>
                    )}
                    {!classifying && (
                    <AnimatePresence mode="wait" initial={false}>
                        <motion.div
                            key={tab}
                            initial={{ opacity: 0, y: 8 }}
                            animate={{ opacity: 1, y: 0 }}
                            exit={{ opacity: 0, y: -6 }}
                            transition={{ duration: 0.18, ease: "easeOut" }}
                            className="flex w-full items-center justify-between"
                        >
                            {(tab === "remove" || tab === "keep") && (
                                <>
                                    <LinkBtn chevron onClick={() => onOpenList(tab)}>
                                        {t("convert.show-count", "查看全部 {{count}} 项{{kind}}清单", {
                                            count:
                                                tab === "remove" ? counts.remove : counts.keep,
                                            kind: tab === "remove" ? t("convert.removed", "剔除") : t("convert.kept", "保留"),
                                        })}
                                    </LinkBtn>
                                    {/* 自动分类出口：进页已默认跑过，这里只给重跑与回退手动改动的入口；
                                        判不出两端的行已归进剔除清单，这里给一句汇总。
                                        回看态整组摘掉，右槽换成全页唯一一句只读说明——
                                        灰化已经说了「点不动」，这句只回答「为什么」，不再给每枚控件挂气泡 */}
                                    {readOnly ? (
                                        <span className="shrink-0 text-[11px] leading-[16px] text-text-3">
                                            {t("convert.read-only", "只读快照 · 不可修改")}
                                        </span>
                                    ) : (
                                        <div className="flex min-w-0 items-center gap-2.5">
                                            {/* 两枚计数提示已收进弹窗：待确认数 = 剔除清单的「需人工确认」tab，
                                                同装数 = 保留清单每行说明尾部，卡底只留操作 */}
                                            <LinkBtn size="sm" onClick={() => onReclassify?.()}>
                                                {t("convert.reclassify", "重新自动分类")}
                                            </LinkBtn>
                                            {manualEdits > 0 &&
                                                (confirmClear ? (
                                                    <>
                                                        <LinkBtn
                                                            size="sm"
                                                            className="text-redstone"
                                                            onClick={() => onClearEdits?.()}
                                                        >
                                                            {t("convert.confirm-clear", "确认清空 {{count}} 项", {
                                                                count: manualEdits,
                                                            })}
                                                        </LinkBtn>
                                                        <LinkBtn
                                                            size="sm"
                                                            className="text-text-3"
                                                            onClick={() => onConfirmClear?.(false)}
                                                        >
                                                            {t("common.cancel", "取消")}
                                                        </LinkBtn>
                                                    </>
                                                ) : (
                                                    <LinkBtn
                                                        size="sm"
                                                        className="text-text-2"
                                                        onClick={() => onConfirmClear?.(true)}
                                                    >
                                                        {t("convert.clear-edits", "清空我的修改")}
                                                    </LinkBtn>
                                                ))}
                                        </div>
                                    )}
                                </>
                            )}
                            {tab === "add" && (
                                <>
                                    <div className="flex min-w-0 items-center gap-2.5">
                                        {counts.addTotal > 0 && (
                                            <LinkBtn chevron onClick={() => onOpenList("add")}>
                                                {t("convert.show-count-added", "查看全部 {{count}} 项新增清单", {
                                                    count: counts.addTotal,
                                                })}
                                            </LinkBtn>
                                        )}
                                    </div>
                                    {/* 两枚 h32 添加按钮：有清单时居右，空方案时整行居中；回看态摘掉 */}
                                    {!readOnly && (
                                        <div
                                            className={cn(
                                                "flex items-center gap-2.5",
                                                counts.addTotal === 0 &&
                                                    "w-full justify-center"
                                            )}
                                        >
                                            <Btn
                                                size="sm"
                                                icon={File}
                                                className="px-[18px] font-semibold text-text-1"
                                                onClick={() => onAddLocal?.()}
                                            >
                                                {t("convert.add-local", "从本地添加")}
                                            </Btn>
                                            <Btn
                                                variant="primary"
                                                size="sm"
                                                icon={Globe}
                                                className="px-[18px] font-semibold"
                                                onClick={() => onAddOnline?.()}
                                            >
                                                {t("convert.add-online", "从网络添加")}
                                            </Btn>
                                        </div>
                                    )}
                                </>
                            )}
                        </motion.div>
                    </AnimatePresence>
                    )}
                </div>
            </div>
        </Panel>
    );
}
