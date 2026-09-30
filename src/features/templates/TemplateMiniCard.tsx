/**
 * 转换页右栏的「配置模板」迷你卡：本次转换选用哪套参数，坐在「转换摘要」之上。
 * 套用语义：只把模板里有的那几项写进转换页，未纳入的保持当前值；**没有「默认模板」这一档**，每次转换都手动点一次。
 * 档位读数由说明行说话（勾=已套用、金三角=套用后改过、灰 Info=未套用），卡头标题前不摆符号。
 * 说明行与动作行的显隐各挂一枚 `Collapse`（`gap={10}` = 本卡 Panel gap），防卡高硬跳带累摘要卡重排；全站规矩见 @/components/ui/Collapse。
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
    Collapse,
    LinkBtn,
    NoteRow,
    Panel,
    PanelHead,
    SearchSelect,
    type SelectOption,
} from "@/components/ui";
import { guardTemplateCap, useTemplateTable } from "./use-template-table";

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

    /** 下拉里只报名字（2026-09-30 他点名撤掉「· N 项」）：档位是列表页那一列的事，这一格要说的是「用哪一份」 */
    const choices: SelectOption[] = templates.map((x) => ({
        value: x.id,
        label: x.name,
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

    /** 「存为模板…」与零模板那颗入口同一条路：带着屏幕上的值去新建页，勾哪行才算进模板。
     *  到档先拒再走，别让人填完整张表单才被告知建不了 */
    const saveAsNew = () => {
        if (!options || !guardTemplateCap(templates.length)) return;
        navigate("template", { seed: templateSeedOf(options) });
    };

    /** 两行各自在场与否：四档之间换的就是这两行的组合，所以各挂一枚 Collapse */
    const showNote = ready && (!!applied || templates.length > 0);
    const showActions = ready && (!!applied || templates.length === 0);

    return (
        <Panel gap={10} padY={12}>
            <PanelHead
                title={t("templates.mini-title", "配置模板")}
                right={
                    <LinkBtn size="sm" onClick={() => switchPrimary("templates")}>
                        {t("templates.manage", "管理")}
                    </LinkBtn>
                }
            />
            <SearchSelect
                className="w-full"
                panelFit
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
            <Collapse when={showNote} gap={10}>
                <NoteRow
                    icon={drift.length > 0 ? TriangleAlert : applied ? Check : Info}
                    tone={drift.length > 0 ? "gold" : applied ? "ok" : undefined}
                >
                    {applied
                        ? drift.length > 0
                            ? t("templates.drift-note", "套用后改过 {{count}} 项 · {{names}}", {
                                  count: drift.length,
                                  names: driftNames,
                              })
                            : t("templates.applied-note", "已套用 · 覆写 {{count}} 项，其余不动", { count })
                        : t("templates.pick-hint", "只覆写模板内配置的选项")}
                </NoteRow>
            </Collapse>
            <Collapse when={showActions} gap={10}>
                <div className="flex items-center gap-3.5">
                    {applied ? (
                        drift.length > 0 ? (
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
                                    {t("templates.save-as", "存为模板")}
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
                        )
                    ) : (
                        <LinkBtn size="sm" onClick={saveAsNew}>
                            {t("templates.new-template", "新建模板")}
                        </LinkBtn>
                    )}
                </div>
            </Collapse>
        </Panel>
    );
}
