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
 *     读出来就是「卡顿一下」。所以内边距与线一律画在壳内的行上。
 *  2. **`gap` 要传父级的格距**：父级是 flex 列时，元素在场=占一格 gap、卸载=少一格，只演 `height`
 *     会在收尾那一帧硬跳 gap 像素（14px 的卡就是 14px）。这里同步演 `marginTop: 0 ↔ −gap` 把自己那格收掉，
 *     与首/中/末位无关。父级不是 flex 列（网格、`gap={0}` 的 divide-y 卡）传 0。
 *  3. **`overflow` 只在演的时候挂**：常驻 `overflow:hidden` 会把浮在内容之上的 `Tip` 气泡裁掉
 *     （运行环境卡那行 Java 全路径就是 Tip）。冷启动那一帧本来就是展开态的不演动画（`AnimatePresence initial={false}`
 *     压掉的正是它），那一态也不能裁 ⇒ 裁剪位用「本实例首帧之后才有的开合」来置位，演完撤。
 *
 * 管不到的一处，用之前先想清楚：挂载之后**内容自己变高**（提示行从「正在检测…」换成完整一句、或折行多一行）
 * 仍然硬跳——终态是 `height:auto`，之后再长不经过这里。要管得挂 ResizeObserver，另立一轮。
 */
import { AnimatePresence, motion } from "motion/react";
import { useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { COLLAPSE } from "@/lib/page-motion";
import { cn } from "@/lib/utils";

export function Collapse({
    when,
    gap = 0,
    className,
    children,
}: {
    /** 展开与否。关掉 = 收起并卸载，不留隐藏 DOM */
    when: boolean;
    /** 父级 flex 列的格距（px）：见文件头第 2 条。不是 flex 列传 0 */
    gap?: number;
    className?: string;
    children: ReactNode;
}) {
    const [clipping, setClipping] = useState(false);
    /** 本实例的首帧：这一帧就在场的块不演动画，也就不该裁（Tip 会被裁走） */
    const firstPaint = useRef(true);

    /* 必须在 paint 之前置位：晚一帧就会看到「零高但不裁剪」的内容盖住下面那行 */
    useLayoutEffect(() => {
        if (firstPaint.current) {
            firstPaint.current = false;
            return;
        }
        setClipping(true);
    }, [when]);

    return (
        <AnimatePresence initial={false}>
            {when && (
                <motion.div
                    key="collapse"
                    initial={{ height: 0, marginTop: -gap, opacity: 0 }}
                    animate={{ height: "auto", marginTop: 0, opacity: 1 }}
                    exit={{ height: 0, marginTop: -gap, opacity: 0 }}
                    transition={COLLAPSE}
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
