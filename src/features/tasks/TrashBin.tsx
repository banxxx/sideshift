/**
 * 回收站入口：右下角垃圾桶，只挂在任务列表页（App.tsx 按 entry.key 渲染）。
 * 短按开弹窗（逐条撤回）、长按清空整个回收站；两者互斥，一次按下只能有一个结果。
 * 「只在删过东西才出现」由数据源保证（Rust 侧只存内存，关应用即清空）；按钮一直挂着、空时透明不吃点击——飞行卡片要拿它的中心当落点，不能量还没渲染的元素。
 */
import { Trash2 } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { Btn, ModalShell } from "@/components/ui";
import { useT } from "@/lib/i18n";
import { notify } from "@/lib/notify";
import { errOf } from "@/lib/errors";
import { clearDeleted, useTrash } from "@/lib/trash-store";
import { cn } from "@/lib/utils";
import { TrashList } from "./TrashModal";

/** 长按时长（ms）：fill 要走完这一整段才算数。给到 1.8s 是「按住了在等一件破坏性操作」的量级 */
const HOLD_MS = 1800;
/** 匀速段走完这些时间、灌到这些进度（后者 ÷ 前者就是那一段的速率） */
const HOLD_SPLIT_T = 0.7;
const HOLD_SPLIT_P = 0.72;
/** 两段：**前段严格匀速**（`linear`，进度随时问是一条直线，不再一按下去就冲掉半格），
 *  **后段越来越慢**收尾。后段用的曲线起始斜率刻意等于匀速段的速率（y1/x1 ≈ 两段速率比），
 *  所以接缝处不换挡、读不出顿感；末段斜率为 0 ⇒ 最后那一截是漫上去的。
 *  WAAPI 的分段 easing 挂在起始关键帧上，正好按这个表逐段生效。 */
const HOLD_STEPS: Array<[at: number, to: number, easing: string]> = [
    [0, 0, "linear"],
    [HOLD_SPLIT_T, HOLD_SPLIT_P, "cubic-bezier(.3,.33,.5,1)"],
    [1, 1, "linear"],
];
/** 由步进表烘出关键帧：clip-path 的右边距从 100% 收到 0 就是「从左往右灌满」 */
const HOLD_KEYS: Keyframe[] = HOLD_STEPS.map(([at, to, easing]) => ({
    offset: at,
    clipPath: `inset(0 ${(1 - to) * 100}% 0 0)`,
    easing,
}));
/** 点击判定的上限（ms）：按下超过这个时长就是「一次按住」，松手不该再开弹窗。
 *  350 是慢速点击与「按住没按满」之间的分界——350 到 HOLD_MS 之间刻意什么都不做，
 *  那正是用户想长按又改主意的时刻，弹个窗出来比不响应更烦。 */
const CLICK_MAX_MS = 350;

/** 收到一条删除时的压弹（样片 squash 那组：先瘪、再鼓过头、回落） */
const SQUASH: Keyframe[] = [
    { transform: "scale(1)" },
    { transform: "scale(0.86) rotate(-6deg)", offset: 0.26 },
    { transform: "scale(1.12) rotate(3deg)", offset: 0.58 },
    { transform: "scale(1)" },
];

export function TrashBin() {
    const t = useT();
    const entries = useTrash();
    const count = entries.length;
    const shown = count > 0;
    const [open, setOpen] = useState(false);
    const [holding, setHolding] = useState(false);
    const btnRef = useRef<HTMLButtonElement>(null);
    /** 长按填充的两层：底层的暗红底 + 上层的红石色图标，靠同一个 clip-path 同步掀开 */
    const boxRef = useRef<HTMLSpanElement>(null);
    const fillRef = useRef<HTMLSpanElement>(null);
    /** 长按这一下的全部动画（底+图标）：松手要一起 reverse，读不到进度只能靠句柄 */
    const holdAnims = useRef<Animation[]>([]);
    const timerRef = useRef(0);
    /** 这次按下的起始时刻：click 不带时长信息，只能自己记（决定它算点击还是算按住） */
    const downAt = useRef(0);
    const lastCount = useRef(count);

    useEffect(() => {
        if (count > lastCount.current) {
            btnRef.current?.animate(SQUASH, {
                duration: 420,
                easing: "cubic-bezier(.22,1.2,.36,1)",
            });
        }
        lastCount.current = count;
    }, [count]);

    useEffect(() => () => window.clearTimeout(timerRef.current), []);

    const empty = async () => {
        try {
            const n = await clearDeleted();
            notify(t("tasks.trash-emptied-count", "已清空回收站，{{count}} 条记录的暂存文件一并删除", { count: n }), "info");
        } catch (e) {
            notify(
                t("tasks.couldn-empty", "清空回收站失败：{{reason}}", { reason: errOf(e) }),
                "error"
            );
        }
    };

    /** 起一次长按填充：底层暗红与红石色图标共用同一张步进表，两层不同步就会被看出来是贴上去的 */
    const runHold = () => {
        holdAnims.current = [
            boxRef.current?.animate(HOLD_KEYS, { duration: HOLD_MS, fill: "forwards" }),
            fillRef.current?.animate(HOLD_KEYS, { duration: HOLD_MS, fill: "forwards" }),
        ].filter((a): a is Animation => !!a);
    };

    const holdStart = (e: React.PointerEvent) => {
        if (!shown || e.button !== 0) return;
        downAt.current = performance.now();
        setHolding(true);
        runHold();
        timerRef.current = window.setTimeout(() => {
            setHolding(false);
            // 动画直接作废（回到行内样式里的「全裁掉」态），下一次按下去才是从头走；
            // 句柄清空，随后的松手才不会对已 cancel 的动画 reverse
            holdAnims.current.forEach((a) => a.cancel());
            holdAnims.current = [];
            void empty();
        }, HOLD_MS);
    };

    /** 松手：没按满就把填充原路退回去（两段一起退，只退一层会出现底色与图标错开的残影）；长按已生效则只收尾 */
    const holdEnd = () => {
        window.clearTimeout(timerRef.current);
        if (!holding) return;
        setHolding(false);
        const anims = holdAnims.current;
        if (!anims.length) return;
        holdAnims.current = [];
        anims.forEach((anim) => {
            if (anim.playState === "finished") anim.cancel();
            else {
                // 松手是「作废」不是「倒放」：加速退回，否则按满 900ms 的时长会原样再等一遍，
                // 那个等待会让人以为是自己松手松晚了
                anim.playbackRate = 2.5;
                anim.reverse();
                anim.onfinish = () => anim.cancel();
            }
        });
    };

    return (
        <>
            <button
                ref={btnRef}
                type="button"
                data-trash-bin=""
                aria-label={t("tasks.trash", "回收站")}
                aria-hidden={!shown}
                tabIndex={shown ? 0 : -1}
                className={cn(
                    "fixed right-6 bottom-6 z-40 flex size-14 items-center justify-center rounded-[18px]",
                    "border bg-surface text-text-2 shadow-[0_10px_26px_rgba(0,0,0,.18)]",
                    "transition-[opacity,scale,border-color,color,background-color] duration-[260ms] ease-out",
                    // 长按只改描边：底色与红石色都由下面两层 clip-path 逐段掀开，
                    // 这里若一按下就整体变色，填充就看不出「正在攒」了（进度信号会先于动画到达）
                    holding ? "border-redstone" : "border-stroke",
                    shown ? "opacity-100 scale-100" : "pointer-events-none opacity-0 scale-[0.6]"
                )}
                onPointerDown={holdStart}
                onPointerUp={holdEnd}
                onPointerLeave={holdEnd}
                onPointerCancel={holdEnd}
                onClick={() => {
                    /** 只认「按下后不到 CLICK_MAX_MS 就松手」这一种 click。
                     *  键盘 Enter/Space 没有 pointerdown，按下时刻记 0，按短点击放行。 */
                    const held = downAt.current ? performance.now() - downAt.current : 0;
                    downAt.current = 0;
                    if (held > CLICK_MAX_MS) return;
                    setOpen(true);
                }}
            >
                {/* 填充底层：暗红底从左往右漫上来。inset-[2px] 是为了不压住描边，
                    圆角跟着收一档，否则方形 clip 边缘会在圆角里露出一条直边 */}
                <span
                    ref={boxRef}
                    className="pointer-events-none absolute inset-[2px] rounded-[16px] bg-redstone-dim"
                    style={{ clipPath: "inset(0 100% 0 0)" }}
                />
                <Trash2 className="relative size-[22px]" />
                {/* 填充上层：同一位置的副本，被下面那层 grid 盒子裁好范围后从左往右露出来。
                    裁外层盒子而不是裁 svg 本身：HTML 盒子的 clip-path 参考框是 border-box，
                    直接裁替换元素在各家实现上口径不一致 */}
                <span className="pointer-events-none absolute inset-0 grid place-items-center">
                    <span
                        ref={fillRef}
                        className="grid size-[22px] place-items-center"
                        style={{ clipPath: "inset(0 100% 0 0)" }}
                    >
                        <Trash2 className="size-[22px] text-redstone" />
                    </span>
                </span>
                <span className="absolute -top-1.5 -right-1.5 min-w-5 rounded-full bg-accent px-1.5 py-px text-center font-mono text-[10px] leading-[14px] font-semibold text-accent-ink">
                    {count}
                </span>
            </button>

            <ModalShell
                open={open}
                onClose={() => setOpen(false)}
                persistent
                width={560}
                height={420}
                title={t("tasks.trash", "回收站")}
                icon={Trash2}
                sub={t("tasks.count-task", "本次会话删除的 {{count}} 条任务 · 关闭应用后自动清空", { count })}
                footerNote={t("tasks.restore-returns", "撤回会把任务原样放回列表；清空才会删掉它的暂存文件")}
                footerActions={
                    <>
                        <Btn
                            size="xs"
                            variant="danger"
                            disabled={count === 0}
                            onClick={() => void empty()}
                        >
                            {t("tasks.clear", "清空")}
                        </Btn>
                        <Btn size="xs" onClick={() => setOpen(false)}>
                            {t("common.close", "关闭")}
                        </Btn>
                    </>
                }
            >
                <TrashList />
            </ModalShell>
        </>
    );
}
