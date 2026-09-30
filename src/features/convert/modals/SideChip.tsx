/**
 * 端标签芯片（三张清单 + 卡内新增行共用）：判据与 detector 同轴——保留组只说服务端、剔除组只说客户端，不与页签打架。
 * 未判定 = 两端都没答上（旧存档或联网反查未跑完）；warnClient = 新增清单里的客户端行标红。
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
