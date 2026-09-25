/**
 * Collapse —— 「卡内整块条件挂卸」的公共折叠件。
 *
 * 用它的地方：一行提示、一颗开关下面的第二行、整张自检卡——凡是 `{cond && <块/>}` 且这块会带着一张
 * 卡的高度一起变的，都走这里，不要再自己写 `height` 或不写。参数只有一个出处：`COLLAPSE`
 * （@/lib/page-motion），那是从 `.scratch/collapse-proto.html` 里挑定的，别在调用点散着覆盖。
 *
 * 三条规矩都是量出来的，改壳之前先读完：
 *  1. **壳必须是空壳**：不带 padding、不带 border。`box-sizing:border-box` 下带 14px 上下内边距的壳
 *     `height:0` 也只能收到 28px（再加 1px 分隔线是 28.67px），末段没得演、演完当场掉那 28px，
 *     读出来就是「卡顿一下」。所以内边距与线一律画在壳内的行上。横向（`axis="x"`）同一条：壳不带上
 *     下 padding 之外也不带左右 padding 与边框，否则 `width:0` 收不到零。
 *  2. **`gap` 要传父级的格距**：父级是 flex 列时，元素在场=占一格 gap、卸载=少一格，只演 `height`
 *     会在收尾那一帧硬跳 gap 像素（14px 的卡就是 14px）。这里同步演 `marginTop: 0 ↔ −gap` 把自己那格收掉，
 *     与首/中/末位无关。父级不是 flex 列（网格、`gap={0}` 的 divide-y 卡）传 0。`axis="x"` 是同一件事
 *     的横版：演 `marginRight: 0 ↔ −gap`。
 *  3. **`overflow` 只在演的时候挂**：常驻 `overflow:hidden` 会把浮在内容之上的 `Tip` 气泡裁掉
 *     （运行环境卡那行 Java 全路径就是 Tip）。冷启动那一帧本来就是展开态的不演动画（`AnimatePresence initial={false}`
 *     压掉的正是它），那一态也不能裁 ⇒ 裁剪位用「本实例首帧之后才有的开合」来置位，演完撤。
 *     **置位的判据必须是「`when` 与上一次不同」，不能是「 effect 是第几次跑」**：StrictMode 在开发期把挂载期
 *     的 effect 连跑两遍，数帧那份第二遍就把「一直在场」误判成「后来开的」，而这一态没有动画 ⇒
 *     `onAnimationComplete` 永不来 ⇒ 裁剪永久挂着，Tip 直接消失（实测踩过）。
 *     横向那一格尤其要这条：壳里的控件必须 `shrink-0`，否则宽度收窄时先被 flex 压扁而不是被裁掉。
 *
 * 管不到的一处，用之前先想清楚：挂载之后**内容自己变高**（提示行从「正在检测…」换成完整一句、或折行多一行）
 * 仍然硬跳——终态是 `height:auto`，之后再长不经过这里。要管得挂 ResizeObserver，另立一轮。
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
