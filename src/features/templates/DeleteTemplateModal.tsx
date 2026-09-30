/**
 * 删除模板确认弹窗（列表页与编辑页共用）：模板误删代价比任务大，必须确认。
 * `persistent` 按全站防误触口径（遮罩与 Esc 都不关，只走按钮）；无正文，名称与档位报在副标里。
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
