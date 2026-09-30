import { FileSliders, Plus } from "lucide-react";
import { useT } from "@/lib/i18n";
import { useNavigation } from "@/lib/navigation";
import { Btn } from "@/components/ui";

/** 占位照真实卡排（同一套 p-5 / 36 图标盒 / 双行文字高度 / `gap-2` 行距）：读数到手时高度几乎不动 */
export function LoadingTemplates() {
    return (
        <div className="flex flex-col gap-2">
            {Array.from({ length: 3 }, (_, i) => (
                <div
                    key={i}
                    className="flex items-center gap-3 rounded-[12px] border border-stroke bg-surface p-5"
                >
                    <span className="size-9 shrink-0 animate-pulse rounded-[10px] bg-surface-2" />
                    <span className="flex min-w-0 flex-1 flex-col gap-[3px]">
                        <span className="h-[13px] w-36 animate-pulse rounded bg-stroke" />
                        <span className="h-[10px] w-52 animate-pulse rounded bg-stroke-soft" />
                    </span>
                    <span className="h-[11px] w-9 shrink-0 animate-pulse rounded bg-stroke-soft" />
                    <span className="h-8 w-24 shrink-0 animate-pulse rounded-lg bg-stroke" />
                </div>
            ))}
        </div>
    );
}

/** 空态解剖照搬 EmptyTasks（TasksPage.tsx:389-416），一处不改：图标盒 56 r20、标题等宽 16/24、CTA 同款覆盖 */
export function EmptyTemplates() {
    const t = useT();
    const { navigate } = useNavigation();
    return (
        <div className="flex h-[clamp(400px,70vh,640px)] flex-col items-center justify-center gap-4 rounded-[12px] bg-bg-app px-5 py-10">
            <div className="flex flex-col items-center gap-4">
                <span className="flex size-14 items-center justify-center rounded-2xl bg-surface-2">
                    <FileSliders className="size-6 text-text-3" />
                </span>
                <div className="flex flex-col items-center gap-1">
                    <span className="font-mono text-[16px] leading-[24px] font-semibold text-text-1">
                        {t("templates.none-yet", "还没有模板")}
                    </span>
                    <span className="font-mono text-[12px] leading-[18px] font-normal text-text-3">
                        {t("templates.empty-sub", "在转换页把参数调好，存一份下次直接套用")}
                    </span>
                </div>
            </div>
            <Btn
                variant="primary"
                icon={Plus}
                className="border border-stroke text-[12px] font-medium"
                onClick={() => navigate("template")}
            >
                {t("templates.new-template", "新建模板")}
            </Btn>
        </div>
    );
}
