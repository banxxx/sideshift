/* ---------------- 方案行（mod-row）：勾选框 16 + 名称/版本横排 + 右侧徽章（可删行悬停露出 ×） ---------------- */
import { X } from "lucide-react";
import type { PlanMod } from "@/lib/types";
import { t, useT } from "@/lib/i18n";
import { CheckBox, TagChip, Tip, TIP_TRIGGER, ToneChip } from "@/components/ui";
import { cn } from "@/lib/utils";
import { SideChip } from "./modals";

export function PlanModRow({
    mod,
    badge,
    readOnly,
    onToggle,
    onRemove,
}: {
    mod: PlanMod;
    badge?: React.ReactNode;
    /** 回看态：勾选位当读数显示，不接点击 */
    readOnly?: boolean;
    onToggle: () => void;
    /** 仅用户自行添加的新增行提供显式删除；缺省 = 不渲染 × */
    onRemove?: () => void;
}) {
    // 组件内一律 useT（切语言才重渲染）：它遮住模块级 `t` 的导入，那枚只给下面的 `badgeFor` 用
    const t = useT();
    // 勾选语义：剔除项未勾选；新增行取消勾选 = 停用（行保留），整行压暗表意不参与构建
    const included = mod.disposition !== "remove" && !mod.disabled;

    return (
        <div className={cn("group flex w-full items-center gap-2.5", mod.disabled && "opacity-55")}>
            <CheckBox
                checked={included}
                review={!included && mod.needsReview}
                readOnly={readOnly}
                onChange={onToggle}
            />
            <div className="flex min-w-0 flex-1 items-center gap-2">
                <span
                    className={cn(
                        "truncate font-mono text-[12px] leading-[18px] font-medium",
                        mod.disabled ? "text-text-3" : "text-text-1"
                    )}
                >
                    {mod.name}
                </span>
                <span className="truncate font-mono text-[11px] leading-[16px] font-normal text-text-3">
                    {mod.version}
                    {mod.loader ? ` · ${mod.loader}` : ""}
                </span>
            </div>
            {badge}
            {onRemove && (
                <button
                    onClick={onRemove}
                    aria-label={t("convert.remove-plan", "从方案移除")}
                    className={cn(
                        // 具名组：本行父级已经是无名 `group`（悬停露出 ×），同名会互相误触发
                        TIP_TRIGGER,
                        "size-6 rounded-md text-text-3",
                        "flex shrink-0 items-center justify-center",
                        "opacity-0 transition-[opacity,color,background-color] duration-150",
                        "hover:bg-redstone-dim hover:text-redstone group-hover:opacity-100 focus-visible:opacity-100"
                    )}
                >
                    <X className="size-3" />
                    <Tip label={t("convert.remove-plan", "从方案移除")} />
                </button>
            )}
        </div>
    );
}

/** 卡内行右侧徽章只放「这行有什么特别的」：
 *  缺件行排最前（拿不到字节是这一行最硬的事实，比「要人确认」更该当场看见）；
 *  剔除/保留行 = 自动补齐 > 需人工确认 > 本地，端标签交给「全部清单」弹窗，卡内不重复占宽
 *  （两端必需的保留行可能成批出现，卡底那句汇总才是它们该被看见的方式）；
 *  新增行 = 用户自己塞进来的（误下载、本地乱拿都在这一步），所以当场就要看到它是哪一端 */
export function badgeFor(mod: PlanMod, local: boolean): React.ReactNode {
    if (mod.cfBlocked)
        return <ToneChip tone="redstone" size="sm">{t("convert.missing-file", "拿不到文件")}</ToneChip>;
    if (mod.autoSupplement) return <TagChip>{t("convert.auto-added", "自动补齐")}</TagChip>;
    if (mod.needsReview) return <ToneChip tone="gold" size="sm">{t("lib.needs-review", "需人工确认")}</ToneChip>;
    if (mod.disposition === "add")
        return <SideChip sides={mod} warnClient />;
    if (local) return <TagChip>{t("convert.local", "本地")}</TagChip>;
    return undefined;
}
