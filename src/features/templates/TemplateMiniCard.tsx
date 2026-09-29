/**
 * 转换页右栏的「配置模板」迷你卡（设计稿 `.scratch/templates-page-proto.html` 屏 ③ + 稿外四档）
 *
 * 卡坐在「转换摘要」之上：左栏是"改参数"，右栏这张说的是"这次要用哪套参数"，
 * 于是模板不再和四张配置卡抢同一个视觉层级。规格上只把留白压一档（12/20 内边距、gap 10），
 * 圆角、描边、标题 13/600、控件 32 高全部照旧 ⇒ 仍是同族件。
 *
 * 卡头标题左边那颗 6px 点是唯一的档位读数：空心 = 未套用 · accent 实心 = 已套用 · gold 实心 = 漂移。
 * 空心那颗的描边用 `text-3` 打 65% 而不是 `stroke`——那档灰压在白卡上几乎化掉，档位一读不出等于没有；
 * 而且空心 vs 实心是**形状差**，不只靠颜色分。点位常驻（永远占 6px + 6px 间隙），换档只换画法，标题不跳位。
 *
 * 套用语义：只把模板里有的那几项写进转换页，未纳入的保持当前值（不是恢复默认）。
 * **没有「默认模板」这一档**——模板不会自动生效，每次转换都在这里点一次；
 * 代价是多一次点击，换来的是不会出现「我没点它怎么就改了」。
 */
import { Info, TriangleAlert, Check } from "lucide-react";
import { useEffect, useState } from "react";
import { useNavigation } from "@/lib/navigation";
import { notify } from "@/lib/notify";
import { useT } from "@/lib/i18n";
import {
    TEMPLATE_FIELD_KEYS,
    driftedKeys,
    pickTemplateValues,
    templateFields,
    templateSeedOf,
    templateValueCount,
    type ConversionOptions,
} from "@/lib/types";
import {
    LinkBtn,
    NoteRow,
    Panel,
    PanelHead,
    SearchSelect,
    type SelectOption,
} from "@/components/ui";
import { useTemplateTable } from "./use-template-table";

/** 档位：只画点，不整卡换底色（卡底换色会让一层的底色跟着选择跳，和「金色只跟随改判态」对不上） */
function StateDot({ state }: { state: "idle" | "applied" | "drift" }) {
    return (
        <span
            aria-hidden="true"
            className={
                state === "applied"
                    ? "size-1.5 shrink-0 rounded-full bg-accent"
                    : state === "drift"
                      ? "size-1.5 shrink-0 rounded-full bg-gold"
                      : "size-1.5 shrink-0 rounded-full border border-text-3/65"
            }
        />
    );
}

export function TemplateMiniCard({
    options,
    patch,
}: {
    /** 与本页四张配置卡同一口径：选项首帧还没算出来（null），这一拍不画说明行与动作行 */
    options: ConversionOptions | null;
    patch: (p: Partial<ConversionOptions>) => void;
}) {
    const t = useT();
    const { navigate, switchPrimary } = useNavigation();
    const { templates, loaded, commit } = useTemplateTable();
    /** 套用的是哪一份（只活在这一次转换配置页上：本页的 `options` 本来也是每次现算的） */
    const [appliedId, setAppliedId] = useState<string | null>(null);
    const applied = templates.find((x) => x.id === appliedId);
    /** 模板表与本页选项都到手才算「能动手」：否则下拉里是空的、点套用会写一份空值 */
    const ready = loaded && options !== null;

    // 模板在列表页被删了 ⇒ 当场退回未套用：留一个下拉里选不中的 id，卡上就顶着一串对不上的读数
    useEffect(() => {
        if (appliedId && !applied) setAppliedId(null);
    }, [appliedId, applied]);

    const count = applied ? templateValueCount(applied.values) : 0;
    const drift = applied && options ? driftedKeys(applied.values, options) : [];
    const driftNames = (() => {
        if (drift.length === 0) return "";
        const short = new Map(templateFields().map((m) => [m.key, m.short]));
        return drift.map((k) => short.get(k) ?? k).join("、");
    })();

    const choices: SelectOption[] = templates.map((x) => ({
        value: x.id,
        label: `${x.name} · ${t("templates.n-items", "{{count}} 项", {
            count: templateValueCount(x.values),
        })}`,
    }));

    const applyTemplate = (id: string) => {
        const tpl = templates.find((x) => x.id === id);
        if (!tpl) return;
        patch(tpl.values);
        setAppliedId(id);
        notify(
            t("templates.applied-toast", "已套用「{{name}}」· 覆写 {{count}} 项", {
                name: tpl.name,
                count: templateValueCount(tpl.values),
            }),
            "success"
        );
    };

    /** 把转换页现在的值抄回这一档模板（勾过的字段不变，只换值）：改完参数顺手更新，不必去编辑页重走一遍 */
    const updateTemplate = async () => {
        if (!applied || !options) return;
        const seed = templateSeedOf(options);
        const keys = TEMPLATE_FIELD_KEYS.filter((k) => applied.values[k] !== undefined);
        const values = pickTemplateValues(seed, keys);
        const next = { ...applied, values, updatedAt: Date.now() };
        if (
            await commit(
                templates.map((x) => (x.id === applied.id ? next : x))
            )
        )
            notify(t("templates.updated-toast", "已更新模板 · {{name}}", { name: applied.name }), "success");
    };

    /** 「存为模板…」与零模板那颗入口同一条路：带着屏幕上的值去新建页，勾哪行才算进模板 */
    const saveAsNew = () => {
        if (!options) return;
        navigate("template", { seed: templateSeedOf(options) });
    };

    return (
        <Panel gap={10} padY={12}>
            <PanelHead
                title={t("templates.mini-title", "配置模板")}
                lead={<StateDot state={!ready || !applied ? "idle" : drift.length > 0 ? "drift" : "applied"} />}
                right={
                    <LinkBtn size="sm" onClick={() => switchPrimary("templates")}>
                        {t("templates.manage", "管理…")}
                    </LinkBtn>
                }
            />
            <SearchSelect
                className="w-full"
                value={applied?.id ?? ""}
                options={choices}
                onChange={applyTemplate}
                readOnly={!ready}
                placeholder={
                    !ready
                        ? t("templates.reading", "正在读取模板…")
                        : templates.length === 0
                          ? t("templates.no-template", "无模板")
                          : t("templates.not-applied", "未套用")
                }
            />
            {!ready ? null : templates.length === 0 ? (
                <LinkBtn size="sm" className="self-start" onClick={saveAsNew}>
                    {t("templates.new-template", "新建模板")}
                </LinkBtn>
            ) : applied ? (
                <>
                    <NoteRow
                        icon={drift.length > 0 ? TriangleAlert : Check}
                        tone={drift.length > 0 ? "gold" : "ok"}
                    >
                        {drift.length > 0
                            ? t("templates.drift-note", "套用后改过 {{count}} 项 · {{names}}", {
                                  count: drift.length,
                                  names: driftNames,
                              })
                            : t("templates.applied-note", "已套用 · 覆写 {{count}} 项，其余不动", { count })}
                    </NoteRow>
                    <div className="flex items-center gap-3.5">
                        {drift.length > 0 ? (
                            <>
                                <LinkBtn size="sm" onClick={() => patch(applied.values)}>
                                    {t("templates.restore", "还原到模板")}
                                </LinkBtn>
                                <LinkBtn size="sm" onClick={() => void updateTemplate()}>
                                    {t("templates.update-template", "更新模板")}
                                </LinkBtn>
                            </>
                        ) : (
                            <>
                                <LinkBtn size="sm" onClick={saveAsNew}>
                                    {t("templates.save-as", "存为模板…")}
                                </LinkBtn>
                                {/* 只摘掉「已套用」这层关系，屏幕上的值照旧——它们是当前配置，不是模板的附属品 */}
                                <LinkBtn
                                    size="sm"
                                    className="text-text-3 hover:text-text-1"
                                    onClick={() => setAppliedId(null)}
                                >
                                    {t("templates.clear-applied", "清除套用")}
                                </LinkBtn>
                            </>
                        )}
                    </div>
                </>
            ) : (
                <NoteRow icon={Info}>
                    {t("templates.pick-hint", "每次转换在这里选一次 · 只覆写模板内那几项")}
                </NoteRow>
            )}
        </Panel>
    );
}
