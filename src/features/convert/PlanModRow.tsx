/* ---------------- 方案行（mod-row）：勾选框 16 + 名称/版本横排 + 右侧徽章（可删行悬停露出 ×） ---------------- */
import { X } from "lucide-react";
import type { PlanMod } from "@/lib/types";
import { CheckBox, TagChip, ToneChip } from "@/components/ui";
import { cn } from "@/lib/utils";
import { SideChip } from "./modals";

export function PlanModRow({
    mod,
    badge,
    onToggle,
    onRemove,
}: {
    mod: PlanMod;
    badge?: React.ReactNode;
    onToggle: () => void;
    /** 仅用户自行添加的新增行提供显式删除；缺省 = 不渲染 × */
    onRemove?: () => void;
}) {
    // 勾选语义：剔除项未勾选；新增行取消勾选 = 停用（行保留），整行压暗表意不参与构建
    const included = mod.disposition !== "remove" && !mod.disabled;

    return (
        <div className={cn("group flex w-full items-center gap-2.5", mod.disabled && "opacity-55")}>
            <CheckBox checked={included} review={!included && mod.needsReview} onChange={onToggle} />
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
                    title="从方案移除"
                    className={cn(
                        "flex size-6 shrink-0 items-center justify-center rounded-md text-text-3",
                        "opacity-0 transition-[opacity,color,background-color] duration-150",
                        "hover:bg-redstone-dim hover:text-redstone group-hover:opacity-100 focus-visible:opacity-100"
                    )}
                >
                    <X className="size-3" />
                </button>
            )}
        </div>
    );
}

/** 卡内行右侧徽章只放「这行有什么特别的」：
 *  剔除/保留行 = 自动补齐 > 需人工确认 > 本地，端标签交给「全部清单」弹窗，卡内不重复占宽
 *  （两端必需的保留行可能成批出现，卡底那句汇总才是它们该被看见的方式）；
 *  新增行 = 用户自己塞进来的（误下载、本地乱拿都在这一步），所以当场就要看到它是哪一端 */
export function badgeFor(mod: PlanMod, local: boolean): React.ReactNode {
    if (mod.autoSupplement) return <TagChip>自动补齐</TagChip>;
    if (mod.needsReview) return <ToneChip tone="gold" size="sm">需人工确认</ToneChip>;
    if (mod.disposition === "add")
        return <SideChip sides={mod} warnClient />;
    if (local) return <TagChip>本地</TagChip>;
    return undefined;
}
