/**
 * 首页（SS.pen Home 三帧：Idle `p4rbP` / Ready·完整版 `Em5yO` / Home·Ready `i1k2jT`）
 *
 * 视图状态机（v11 "渐进式披露" + "轨道=当前任务实况小窗" 定稿）：
 * - idle       未选包且无任务：760×360 拖放卡 + 帮助三卡，无轨道
 * - parsing    解析瞬间：紧凑拖放卡 + 骨架详情卡 + 轨道第 1 站金色激活
 * - ready      已选包未开始：拖放卡 + 详情卡 + 轨道全灰起点、日志"等待开始转换"
 * - converting 有活跃/最近任务：同布局，轨道反映任务实况，右下"查看任务详情"
 *
 * 数据来源全部走 @/lib/api 门面（浏览器 dev 自动落 mock），页面零 invoke。
 */
import { useNavigation } from "@/lib/navigation";
import {
    useActiveTask,
    usePackSelection,
    useTauriFileDrop,
} from "@/lib/home-state";
import { taskToRail } from "@/lib/rail-view";
import { Dropzone } from "@/components/features/Dropzone";
import { HelpRow } from "@/components/features/HelpRow";
import { PackCard } from "@/components/features/PackCard";
import { ShiftRail } from "@/components/features/ShiftRail";
import type { PackCardStatus } from "@/components/features/PackCard";
import { PageHeader } from "@/components/design/ui";

export function HomePage() {
    const { navigate } = useNavigation();
    const { manifest, parsing, error, errorName, parse, pickByDialog, reset } =
        usePackSelection();
    const { active } = useActiveTask();

    // Tauri 下 OS 拖入 → 直接解析第一个文件
    useTauriFileDrop((paths) => void parse(paths[0]));

    const converting = !!active && (active.status === "running" || active.status === "queued");
    // 解析失败也要停留在卡片布局，把错误显式呈现出来（不能闪回 idle 装作无事发生）
    // 注意：历史任务（已完成/失败/取消）不算活跃，否则一进首页就会被拽进卡片布局。
    const view = !manifest && !parsing && !converting && !error
        ? "idle"
        : parsing || (!manifest && converting)
          ? "parsing"
          : converting || (!!active && manifest?.fileName === active.pack.fileName)
            ? "converting"
            : "ready";

    const cardStatus: PackCardStatus = parsing
        ? "parsing"
        : error
          ? "error"
          : converting
            ? "converting"
            : "ready";

    const rail = view === "converting" && active ? taskToRail(active) : null;

    return (
        <div className="flex flex-col gap-5 min-h-full">
            <PageHeader
                title="客户端整合包 → 服务端"
                sub="拖入整合包，SideShift 自动剔除客户端专属内容，补齐服务端依赖，生成可直接运行的服务器包。"
            />

            {view === "idle" ? (
                /* Idle：拖放卡 + 帮助三卡，垂直居中（设计稿 760 栏居中） */
                <div className="flex-1 flex flex-col items-center justify-center gap-5 py-2">
                    <Dropzone onPick={pickByDialog} onDropPaths={(p) => void parse(p[0])} />
                    <HelpRow />
                </div>
            ) : (
                <div className="flex flex-col gap-5">
                    {/* 上半：拖放卡（紧凑 540）+ 已选包详情卡，设计稿定高 288 */}
                    <div className="flex gap-5 items-stretch h-[288px]">
                        <Dropzone
                            compact
                            busy={parsing}
                            onPick={pickByDialog}
                            onDropPaths={(p) => void parse(p[0])}
                        />
                        <PackCard
                            manifest={manifest ?? active?.pack ?? null}
                            status={cardStatus}
                            error={error}
                            fileName={errorName ?? undefined}
                            onChangeFile={reset}
                            onPrimary={() =>
                                converting && active
                                    ? navigate("task", { taskId: active.id })
                                    : navigate("convert", { manifest })
                            }
                        />
                    </div>

                    {/* Shift Rail 实况小窗 */}
                    <ShiftRail
                        statuses={
                            rail
                                ? rail.statuses
                                : /* 设计稿 i1k2jT：未开始时第 1 站即为金色"当前站" */
                                  { parser: "active" }
                        }
                        status={
                            rail
                                ? rail.status
                                : parsing
                                  ? { label: "解析中", tone: "gold" }
                                  : { label: "已检测 · 待转换", tone: "emerald" }
                        }
                        logs={rail?.logs ?? []}
                        waiting={
                            rail?.waiting ?? {
                                title: "等待开始转换",
                                detail: `已选择 ${manifest?.fileName ?? ""} · 点击「配置并转换」进入转换配置`,
                            }
                        }
                        onOpenTask={
                            active ? () => navigate("task", { taskId: active.id }) : undefined
                        }
                    />
                </div>
            )}
        </div>
    );
}
