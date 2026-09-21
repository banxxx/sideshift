/**
 * 侧栏底部提示区（版本号/主题行上方的预留槽位）：全局 notify() 提示的唯一渲染出口。
 *
 * 位置钉死在侧栏底部、向上生长：新提示贴最近底缘入场（自下淡入+微缩放），
 * 旧提示被顶上去时靠 layout 动画平滑位移；移除用 popLayout 让其余即时补位。
 * 卡片走「静态悬停壳」口径：surface 底 + stroke 描边，kind 决定图标与语义色，
 * 悬停露出 × 可提前关闭（错误类停留更久，给用户确认余地）。
 */
import { AnimatePresence, motion } from "motion/react";
import { AlertTriangle, CircleCheck, CircleX, Info, X } from "lucide-react";
import { cn } from "@/lib/utils";
import { dismissNotice, useNotices, type Notice, type NoticeKind } from "@/lib/notify";

const KIND_STYLE: Record<
    NoticeKind,
    { icon: typeof Info; tone: string; surface: string }
> = {
    info: { icon: Info, tone: "text-accent", surface: "bg-accent-dim" },
    success: { icon: CircleCheck, tone: "text-emerald", surface: "bg-emerald-dim" },
    warn: { icon: AlertTriangle, tone: "text-gold", surface: "bg-gold-dim" },
    error: { icon: CircleX, tone: "text-redstone", surface: "bg-redstone-dim" },
};

function NoticeCard({ notice }: { notice: Notice }) {
    const { icon: Icon, tone, surface } = KIND_STYLE[notice.kind];
    return (
        <motion.div
            layout
            initial={{ opacity: 0, y: 12, scale: 0.97 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, y: 6, scale: 0.97 }}
            transition={{ duration: 0.2, ease: "easeOut" }}
            className="group flex items-start gap-2 rounded-lg border border-stroke bg-surface px-2.5 py-2"
        >
            {/* 18px 语义色淡底图标盒，与列表行图标盒同口径 */}
            <span
                className={cn(
                    "mt-px flex size-[18px] shrink-0 items-center justify-center rounded-[5px]",
                    surface
                )}
            >
                <Icon className={cn("size-3", tone)} />
            </span>
            {/* break-words 是硬要求：提示里常带路径/文件名这类无空格长串（`C:\Users\...\output`、
             *  jar 名），默认不断行的词会直接顶穿卡片描边 */}
            <span className="min-w-0 flex-1 break-words text-[11px] leading-[16px] text-text-2">
                {notice.text}
            </span>
            <button
                onClick={() => dismissNotice(notice.id)}
                title="关闭提示"
                className={cn(
                    "-mr-0.5 -mt-0.5 flex size-4 shrink-0 items-center justify-center rounded text-text-3",
                    "opacity-0 transition-opacity duration-150 hover:text-text-1 group-hover:opacity-100"
                )}
            >
                <X className="size-3" />
            </button>
        </motion.div>
    );
}

export function NotificationStack() {
    const notices = useNotices();
    return (
        /* 容器常驻（侧栏已有 px-3，勿再加横向内边距）：空态零高度，退场由内层 popLayout 补位 */
        <div className={cn("flex flex-col gap-2", notices.length > 0 && "pb-2.5")}>
            <AnimatePresence mode="popLayout" initial={false}>
                {notices.map((n) => (
                    <NoticeCard key={n.id} notice={n} />
                ))}
            </AnimatePresence>
        </div>
    );
}
