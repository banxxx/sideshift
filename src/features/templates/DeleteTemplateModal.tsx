/**
 * 删除模板确认（设计稿 `.scratch/templates-page-proto.html` 稿外件 ③）
 *
 * 走弹窗而不是直接删：模板是攒出来的配置，误删代价比任务大（任务还能重转，模板得重新勾）。
 * 弹窗没有正文——名称与档位就在副标里报完，脚部那句说清「删了不影响什么」。
 * `persistent` 按全站防误触口径：遮罩与 Esc 都不关，只走按钮。
 * 列表页与编辑页共用这一枚：两处点删除必须看到同一个对话框。
 */
import { Trash2 } from "lucide-react";
import { useT } from "@/lib/i18n";
import { templateValueCount, type ConversionTemplate } from "@/lib/types";
import { Btn, ModalShell } from "@/components/ui";

export function DeleteTemplateModal({
    template,
    onClose,
    onConfirm,
}: {
    template: ConversionTemplate | null;
    onClose: () => void;
    onConfirm: (tpl: ConversionTemplate) => void;
}) {
    const t = useT();
    return (
        <ModalShell
            open={template !== null}
            onClose={onClose}
            width={400}
            persistent
            iconNode={
                <span className="flex size-10 shrink-0 items-center justify-center rounded-lg bg-surface-2">
                    <Trash2 className="size-5 text-redstone" />
                </span>
            }
            title={t("templates.delete-title", "删除模板")}
            sub={
                template
                    ? t("templates.delete-sub", "{{name}} · {{count}} 项", {
                          name: template.name,
                          count: templateValueCount(template.values),
                      })
                    : undefined
            }
            footerNote={t("templates.delete-foot", "只删这份配置 · 已建任务与产物不受影响")}
            footerActions={
                <>
                    <Btn size="xs" onClick={onClose}>
                        {t("common.cancel", "取消")}
                    </Btn>
                    <Btn
                        size="xs"
                        variant="danger"
                        disabled={template === null}
                        onClick={() => template && onConfirm(template)}
                    >
                        {t("templates.delete", "删除")}
                    </Btn>
                </>
            }
        >
            {null}
        </ModalShell>
    );
}
