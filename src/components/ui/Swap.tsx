/**
 * Swap —— 「同一位置换一批 DOM」的外壳：状态按钮组、清单↔空态这类整块替换。
 *
 * 换页签（TAB_SWEEP）走的是「往旁边翻一页」，这里走的是「新的一批落位」，所以节拍更小、纵向走。
 * 两者语义不同，别拿 Swap 去装页签内容。
 *
 * 为什么是 `mode="popLayout"`：`mode="wait"` 要等旧层演完才挂新层，中间容器高度塌一次
 * （按钮组从两条变一条就是一下蹿上去再落回）。popLayout 让旧层当场脱离文档流，新层立刻占位，
 * 观感是一次同位溶解。
 *
 * 为什么本组件自带一层 `relative`：motion 给退场层注入的是 `position:absolute` + 量好的
 * top/left/width/height，而这两个坐标按「最近定位祖先」算。父级没定位时它会一路找到 body，
 * 量到的坐标与落点用的坐标系对不上，残影就盖到别处去了（任务列表那轮退场层被打回就是这么来的）。
 *
 * `initial={false}`：外壳首次挂载不算换内容——冷启动那一帧不该演一遍进出场。
 */
import { AnimatePresence, motion } from "motion/react";
import type { ReactNode } from "react";
import { SWAP } from "@/lib/page-motion";
import { cn } from "@/lib/utils";

export function Swap({
    swapKey,
    className,
    children,
}: {
    /** 换内容就换这个 key（同一 key 下 children 自己变化不演动画，例如按钮文案改写） */
    swapKey: string | number;
    /** 层内排布：默认竖排等宽（按钮组/清单），横排自己给 */
    className?: string;
    children: ReactNode;
}) {
    return (
        <div className="relative">
            <AnimatePresence mode="popLayout" initial={false}>
                <motion.div
                    key={swapKey}
                    variants={SWAP}
                    initial="hidden"
                    animate="show"
                    exit="exit"
                    className={cn("flex w-full flex-col gap-2.5", className)}
                >
                    {children}
                </motion.div>
            </AnimatePresence>
        </div>
    );
}
