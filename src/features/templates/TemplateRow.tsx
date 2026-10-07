import { Copy, FileSliders, GripVertical, Pencil, Trash2 } from "lucide-react";
import { useT } from "@/lib/i18n";
import { templateValueCount, type ConversionTemplate } from "@/lib/types";
import { IconBtn } from "@/components/ui";
import { cn } from "@/lib/utils";

/**
 * 抬起层那三枚按钮禁的是「键盘还能 Tab 进去」（那层已 `aria-hidden`，可聚焦的 child 站在隐形层上），
 * 不是「它们不能按」。所以把 `IconBtn` 自带的 `disabled:opacity-40` 压回原样——
 * 跟手那张必须和流内那张一模一样，他否决过任何让拖影看起来不同的做法。
 */
const LIFT_INERT = "disabled:opacity-100";

/**
 * 一张模板卡。两处复用同一份 markup：列表里那张（可拖、有三枚动作）与抬起层那张（`overlay`，
 * 只多一层投影、不吃命中、动作按钮不再挂着），所以「跟手的是这张卡本体」这条读感才立得住。
 */
export function TemplateRow({
    template,
    lifted,
    overlay,
    onOpen,
    onCopy,
    onDelete,
    onGripDown,
}: {
    template: ConversionTemplate;
    /** 这张已经脱离显示序列、由抬起层代管：格子留着（容器高度恒定才不跳版），卡本身收起来 */
    lifted?: boolean;
    /** 抬起层：投影 + 不吃命中，动作件不再要交互 */
    overlay?: boolean;
    onOpen?: () => void;
    onCopy?: () => void;
    onDelete?: () => void;
    onGripDown?: (e: React.PointerEvent<HTMLElement>) => void;
}) {
    const t = useT();
    const count = templateValueCount(template.values);
    return (
        <section
            className={cn(
                // 卡身没有 hover 档：整卡不可点，hover 压深会把它读成一个假出口。
                // 材质与任务卡同一家（v5 磨砂）。抬起层不另造面貌：描边与实心面都不是卡这一族的
                // 东西，离地感只由投影给（弹窗档：面比卡厚一点 + 无描边 + 重投影，深浅两档都已在令牌里）。
                overlay
                    ? "modal-frost group flex items-center gap-3 rounded-[12px] p-5"
                    : "card-frost group flex items-center gap-3 rounded-[12px] p-5",
                lifted && "invisible"
            )}
        >
            {/* 拖动把手：命中区**吃满整卡高度**（`self-stretch`，实测 79px 而不是原来那 32px 小格）——
                它是唯一的拖动入口，命中区太小就直接读成「拖不动」。`touch-none` 让指针不被系统滚动手势吃掉，
                `draggable={false}` 挡掉「从图标起步拖出原生拖拽」那一路（它一发 `pointercancel` 手势就归它了）。
                这一按之后**不再挂任何 move/up**：整趟监听都挂在 `window` 上，见 `onGripDown`。
                整卡 draggable 那条已经删了——Tauri 的 OS 拖入拦截开着，dragstart 本来就不会来。 */}
            <span
                onPointerDown={onGripDown}
                draggable={false}
                className={cn(
                    "flex w-5 shrink-0 self-stretch touch-none items-center justify-center rounded-lg text-text-3",
                    "transition-colors group-hover:text-text-2",
                    !overlay && "cursor-grab active:cursor-grabbing"
                )}
            >
                <GripVertical className="size-3.5" />
            </span>
            <span className="flex size-9 shrink-0 items-center justify-center rounded-[10px] bg-accent-dim text-accent">
                <FileSliders className="size-[17px]" />
            </span>
            <span className="flex min-w-0 flex-1 flex-col gap-[3px]">
                <span className="truncate font-mono text-[13px] leading-[20px] font-semibold text-text-1">
                    {template.name}
                </span>
                <span className="truncate font-mono text-[11px] leading-[16px] font-normal text-text-3">
                    {/* 没备注就是没有备注：一个短横足够，「未填备注」四个字会把这一行读成一句状态 */}
                    {template.note || "-"}
                </span>
            </span>
            {/* 两轨分界：左边「这张卡叫什么」，右边「它收了几档 + 能做什么」 */}
            <span className="h-7 w-px shrink-0 bg-stroke-soft" />
            <span className="min-w-[34px] shrink-0 text-right font-mono text-[11px] leading-[16px] font-normal text-text-3">
                {t("templates.n-items", "{{count}} 项", { count })}
            </span>
            <span className="flex shrink-0 items-center gap-2">
                <IconBtn
                    icon={Pencil}
                    title={t("templates.edit", "编辑")}
                    className={LIFT_INERT}
                    disabled={overlay}
                    onClick={onOpen}
                />
                <IconBtn
                    icon={Copy}
                    title={t("templates.duplicate", "复制")}
                    className={LIFT_INERT}
                    disabled={overlay}
                    onClick={onCopy}
                />
                <IconBtn
                    icon={Trash2}
                    title={t("templates.delete", "删除")}
                    className={cn("hover:bg-redstone-dim hover:text-redstone", LIFT_INERT)}
                    disabled={overlay}
                    onClick={onDelete}
                />
            </span>
        </section>
    );
}
