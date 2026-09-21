/**
 * 端标签芯片（三张清单 + 卡内新增行共用）：服务端必装 / 服务端可选 / 客户端必装 / 客户端可选 / 需人工确认。
 * 判据与 detector 同轴，所以保留组的标签只说服务端、剔除组的标签只说客户端，不会与页签打架。
 * warnClient = 新增清单里的客户端行标红（误下载最典型的一种）
 */
import { TagChip } from "@/components/ui";
import { sideTagLabel, sideTagOf } from "@/lib/format";
import type { PlanMod } from "@/lib/types";

export function SideChip({
    sides,
    square,
    warnClient,
}: {
    sides: Pick<PlanMod, "clientSide" | "serverSide">;
    square?: boolean;
    warnClient?: boolean;
}) {
    const tag = sideTagOf(sides);
    const clientSide = tag === "clientRequired" || tag === "clientOptional";
    return (
        <TagChip
            square={square}
            tone={
                tag === "review" ? "gold" : warnClient && clientSide ? "redstone" : undefined
            }
        >
            {sideTagLabel(tag)}
        </TagChip>
    );
}
