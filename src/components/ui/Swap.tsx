/**
 * Swap —— 「同一位置换一批 DOM」的外壳：状态按钮组、清单↔空态这类整块替换。
 * 换页签走 TAB_SWEEP（往旁边翻一页），语义不同，别拿 Swap 去装页签内容。
 * `mode="popLayout"`：旧层当场脱离文档流、新层立刻占位，容器高度不塌（`wait` 会塌一次）。
 * 本组件自带一层 `relative`：motion 给退场层的绝对坐标按最近定位祖先算，父级没定位会一路找到 body、残影盖到别处；`initial={false}` 让外壳首次挂载不演进出场。
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
