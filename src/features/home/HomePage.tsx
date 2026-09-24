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
import { AnimatePresence, motion, type Variants } from "motion/react";
import { LineDotRightHorizontal } from "lucide-react";
import { useActiveTask, useTauriFileDrop } from "@/features/home/home-state";
import { usePackStore } from "@/lib/pack-store";
import { taskToRail } from "@/lib/rail-view";
import { truncateMiddle } from "@/lib/format";
import { Dropzone } from "@/features/home/Dropzone";
import { HelpRow } from "@/features/home/HelpRow";
import { PackCard } from "@/features/home/PackCard";
import type { PackCardStatus } from "@/features/home/PackCard";
import { ShiftRail } from "@/components/shared/ShiftRail";
import { PageHeader } from "@/components/ui";
import { MORPH, RAIL_RISE } from "@/lib/springs";

/** idle 分支容器：具名 variants 把 show/hide 标签向后代（HelpRow 三卡）传播；
 *  入场延迟 120ms 让 Dropzone 先行，退场整体 0.18 淡出与三卡下沉同步发生 */
const IDLE_BRANCH: Variants = {
    hidden: {},
    show: { transition: { delayChildren: 0.12 } },
    hide: { opacity: 0, transition: { duration: 0.18 } },
};

export function HomePage() {
    const { navigate } = useNavigation();
    const { manifest, parsing, error, errorName, selectedAt, parse, pickByDialog, reset } =
        usePackStore();
    const { active, ready } = useActiveTask();

    // Tauri 下 OS 拖入 → 直接解析第一个文件；拖拽悬停标志用于拖放卡高亮
    const fileDragging = useTauriFileDrop((paths) => void parse(paths[0]));

    /**
     * 轨道/卡片该反映哪条任务：
     * - 排队或运行中 → 一律反映（轨道是全局实况小窗，同一时刻只有一条在动）；
     * - 已结束（成功/失败/取消）→ 只有「本次选包之后创建」的任务才算数。
     *   重新拖入整合包时 selectedAt 会刷新，于是上一套构建的进度与日志立刻归零，
     *   哪怕两次是同名同路径的包——按文件名比对无法区分，所以比对创建时刻。
     */
    const live = !!active && (active.status === "running" || active.status === "queued");
    const shown = active && (live || active.createdAt >= selectedAt) ? active : null;
    const converting =
        !!shown && (shown.status === "running" || shown.status === "queued");
    // 解析失败也要停留在卡片布局，把错误显式呈现出来（不能闪回 idle 装作无事发生）
    // 注意：与本次选包无关的历史任务（已完成/失败/取消）不算活跃，
    // 否则一进首页就会被拽进卡片布局。
    const view = !manifest && !parsing && !converting && !error
        ? "idle"
        : parsing || (!manifest && converting)
          ? "parsing"
          : converting || (!!shown && !!manifest)
            ? "converting"
            : "ready";

    const cardStatus: PackCardStatus = parsing
        ? "parsing"
        : error
          ? "error"
          : converting
            ? "converting"
            : "ready";

    const rail = view === "converting" && shown ? taskToRail(shown) : null;

    /**
     * 首帧闸门：任务快照还没落地、本地也没有「已选包」的证据 ⇒ 还不知道该摆哪套布局，先留白。
     * `active === null` 单独看有两层意思（确实没任务 / 还没查过），赌错的代价是冷启动正好有任务
     * 在跑时，先画一帧大拖放卡、再被 layoutId 的共享元素动效拽成紧凑卡，看着像界面自己跳了一下。
     * 留白只有一帧（listTasks 走内存），且下面按它整块挂卸 AnimatePresence：
     * 闸门开合不重播已有动画，`initial={false}` 对新实例照样成立。
     */
    const unknown = !ready && !manifest && !parsing && !error;

    return (
        // overflow-hidden：进出场时 PackCard 右移 64px / ShiftRail 下移 72px 属于
        // 容器外变换，不裁剪会撑大 main 的滚动区域、闪出横竖滚动条
        <div className="flex flex-col gap-5 min-h-full relative overflow-hidden py-6">
            <PageHeader
                title={
                    <span className="inline-flex items-center gap-2">
                        客户端
                        <LineDotRightHorizontal
                            aria-hidden
                            className="size-[19px] text-accent"
                            strokeWidth={2.5}
                        />
                        服务端
                    </span>
                }
                sub="拖入整合包，SideShift 自动剔除客户端专属内容，补齐服务端依赖，生成可直接运行的服务器包。"
            />

            {/* idle ↔ cards 布局切换：
                - popLayout 让退场分支脱离文档流，入场布局立即就位、不互相挤压；
                - Dropzone 挂同一 layoutId，motion 自动投影"大卡缩小左上归位"（退出反向）；
                - PackCard 从右滑入、ShiftRail 从下方滑入，错峰 60ms，退场按原路径返回 */}
            {unknown ? (
                <div className="flex-1" />
            ) : (
                <AnimatePresence mode="popLayout" initial={false}>
                    {view === "idle" ? (
                        <motion.div
                            key="idle"
                            className="flex-1 flex flex-col items-center justify-center gap-5 py-2"
                            variants={IDLE_BRANCH}
                            initial="hidden"
                            animate="show"
                            exit="hide"
                        >
                            <Dropzone
                                layoutId="dropzone"
                                onPick={pickByDialog}
                                onDropPaths={(p) => void parse(p[0])}
                                fileDragging={fileDragging}
                            />
                            <HelpRow />
                        </motion.div>
                    ) : (
                        <motion.div key="cards" className="flex flex-col gap-5">
                            {/* 上半：拖放卡（≈58% 宽）+ 已选包详情卡。
                                高度用 36vh 卡在设计稿的 288：1200×800 下 36vh 正好 288（像素级保真），
                                窗口变矮时先收这里，而不是把下面的轨道卡挤出滚动区 */}
                            <div className="flex h-[clamp(256px,36vh,288px)] items-stretch gap-5">
                                <Dropzone
                                    layoutId="dropzone"
                                    compact
                                    busy={parsing}
                                    onPick={pickByDialog}
                                    onDropPaths={(p) => void parse(p[0])}
                                    fileDragging={fileDragging}
                                />
                                <motion.div
                                    className="flex min-w-0 flex-1"
                                    initial={{ x: 64, opacity: 0 }}
                                    animate={{ x: 0, opacity: 1 }}
                                    exit={{ x: 64, opacity: 0, transition: { duration: 0.18 } }}
                                    transition={MORPH}
                                >
                                    <PackCard
                                        manifest={manifest ?? shown?.pack ?? null}
                                        status={cardStatus}
                                        error={error}
                                        fileName={errorName ?? undefined}
                                        onChangeFile={reset}
                                        onPrimary={() =>
                                            converting && shown
                                                ? navigate("task", { taskId: shown.id })
                                                : navigate("convert", { manifest })
                                        }
                                    />
                                </motion.div>
                            </div>

                            {/* Shift Rail 实况小窗 */}
                            <motion.div
                                initial={{ y: 72, opacity: 0 }}
                                animate={{ y: 0, opacity: 1 }}
                                exit={{ y: 72, opacity: 0, transition: { duration: 0.18 } }}
                                transition={{ ...RAIL_RISE, delay: 0.06 }}
                            >
                                <ShiftRail
                                    statuses={
                                        rail
                                            ? rail.statuses
                                            : /* 未开始：第 1 站是"当前站"（灰底 + 很淡的光环） */
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
                                    runFrac={rail?.runFrac}
                                    subs={rail?.subs}
                                    clipHeader={
                                        shown
                                            ? `SideShift 日志 · ${shown.pack.fileName} · ${shown.id} · ${shown.status}`
                                            : undefined
                                    }
                                    waiting={
                                        rail?.waiting ?? {
                                            title: "等待开始转换",
                                            detail: `已选择 ${truncateMiddle(manifest?.fileName ?? "", 40)} · 点击「配置并转换」进入转换配置`,
                                        }
                                    }
                                    onOpenTask={
                                        shown ? () => navigate("task", { taskId: shown.id }) : undefined
                                    }
                                />
                            </motion.div>
                        </motion.div>
                    )}
                </AnimatePresence>
            )}
        </div>
    );
}
