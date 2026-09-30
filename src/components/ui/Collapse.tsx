/**
 * Collapse —— 「卡内整块条件挂卸」的公共折叠件：一行提示、开关下面的第二行、整张自检卡都走这里，别自己写 `height`。
 * 参数单一出处 `COLLAPSE`（@/lib/page-motion），别在调用点散着覆盖。三条硬规矩：
 *  1. **壳必须是空壳**（无 padding/border，否则 `height:0` 收不到零、末段当场掉高度）；内边距与线一律画在壳内的行上。
 *  2. **`gap` 要传父级的格距**：同步演 `marginTop: 0 ↔ −gap` 把自己那格收掉，否则收尾硬跳 gap 像素；父级不是 flex 列传 0（`axis="x"` 演 marginRight）。
 *  3. **`overflow` 只在演的时候挂、演完撤**（常驻会裁掉浮在内容上的 Tip 气泡）；置位判据必须是「`when` 与上一次不同」，不能数 effect 跑了几遍（StrictMode 双跑）。
 * `axis="x"` 时壳里的控件必须 `shrink-0`；挂载后内容自己变高不经此处（终态 `height:auto`）。
 */
import { AnimatePresence, motion } from "motion/react";
import { useLayoutEffect, useRef, useState, type ReactNode } from "react";
import type { TargetAndTransition, Transition } from "motion/react";
import { COLLAPSE } from "@/lib/page-motion";
import { cn } from "@/lib/utils";

export function Collapse({
    when,
    gap = 0,
    axis = "y",
    transition = COLLAPSE,
    className,
    children,
}: {
    /** 展开与否。关掉 = 收起并卸载，不留隐藏 DOM */
    when: boolean;
    /** 父级 flex 的格距（px）：见文件头第 2 条。不是 flex 传 0 */
    gap?: number;
    /** `"x"` = 收放一行里的一格（演 `width` + `marginRight`），默认 `"y"` = 收放一卡里的一行 */
    axis?: "y" | "x";
    /** 只在换节拍时传，且只传 `@/lib/page-motion` 里已有的名字，别在调用点写字面量 */
    transition?: Transition;
    className?: string;
    children: ReactNode;
}) {
    const [clipping, setClipping] = useState(false);
    /** 上一次提交时在场与否。初值取挂载那一刻的 `when` ⇒ 「首帧就在场」天然相等，不置裁剪位 */
    const lastWhen = useRef(when);

    /* 必须在 paint 之前置位：晚一帧就会看到「零高但不裁剪」的内容盖住下面那行 */
    useLayoutEffect(() => {
        if (lastWhen.current === when) return;
        lastWhen.current = when;
        /* 系统要求减少动效时不裁：`MotionConfig reducedMotion="user"`（App.tsx）认的就是这条查询，
           而 motion 把 `height`/`width` 归在 positional 键里（dist:4373）⇒ 那一态 `type:false` 瞬变、
           根本没有"零高但内容溢出"的那几帧可裁，挂上去只会把 Tip 裁走。 */
        if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
        setClipping(true);
    }, [when]);

    /* 两条轴只差「收哪条尺寸、补哪侧外边距」，节拍/淡入淡出/裁剪撤除全部共用 */
    const closed: TargetAndTransition =
        axis === "y" ? { height: 0, marginTop: -gap } : { width: 0, marginRight: -gap };
    const open: TargetAndTransition =
        axis === "y" ? { height: "auto", marginTop: 0 } : { width: "auto", marginRight: 0 };

    return (
        <AnimatePresence initial={false}>
            {when && (
                <motion.div
                    key="collapse"
                    initial={{ ...closed, opacity: 0 }}
                    animate={{ ...open, opacity: 1 }}
                    exit={{ ...closed, opacity: 0 }}
                    transition={transition}
                    onAnimationComplete={() => setClipping(false)}
                    className={cn("min-w-0 shrink-0", className)}
                    style={{ overflow: clipping ? "hidden" : undefined }}
                >
                    {children}
                </motion.div>
            )}
        </AnimatePresence>
    );
}
