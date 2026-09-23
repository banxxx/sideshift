/**
 * 回收站弹窗内容：本次会话删掉的任务，一一条带「撤回」。
 *
 * 刻意做轻：只列包名 + 一行明细 + 状态，不搬整张任务卡（日志、进度、按钮组在回收站里没意义）。
 * 「长按垃圾桶清空」这件事用户猜不到，所以脚注写明，并在页脚给一个等价的「清空」按钮
 * （键盘用户没有长按，这条出口是必需的）。
 */
import { Trash2 } from "lucide-react";
import { Btn, ListRow, ToneChip, type Tone } from "@/components/ui";
import { formatSize, loaderLabel, truncateMiddle } from "@/lib/format";
import { notify } from "@/lib/notify";
import { restoreDeleted, useTrash } from "@/lib/trash-store";
import type { TaskStatus, TrashEntry } from "@/lib/types";

/** 状态芯片：回收站只收终态（运行中的行后端拒绝删除），排队中是唯一的非终态例外 */
const STATUS: Record<TaskStatus, { label: string; tone: Tone }> = {
    queued: { label: "排队中", tone: "muted" },
    running: { label: "转换中", tone: "gold" },
    success: { label: "已完成", tone: "emerald" },
    failed: { label: "已失败", tone: "redstone" },
    cancelled: { label: "已取消", tone: "muted" },
};

/** 相对时间：弹窗开着的几分钟内不会自己跳字，为这个再挂一个定时器不值 */
function ago(ts: number): string {
    const s = Math.max(0, Math.round((Date.now() - ts) / 1000));
    if (s < 60) return `${s} 秒前删除`;
    const m = Math.round(s / 60);
    if (m < 60) return `${m} 分钟前删除`;
    return `${Math.round(m / 60)} 小时前删除`;
}

export function TrashList() {
    const entries = useTrash();
    if (entries.length === 0) {
        return (
            <div className="flex flex-1 flex-col items-center justify-center gap-2 text-text-3">
                <Trash2 className="size-5" />
                <span className="font-mono text-[11px] leading-[16px]">回收站已清空</span>
            </div>
        );
    }
    return (
        <div className="-mx-1 flex min-h-0 flex-1 flex-col gap-1.5 overflow-auto">
            {entries.map((e) => (
                <TrashRow key={e.taskId} entry={e} />
            ))}
        </div>
    );
}

function TrashRow({ entry }: { entry: TrashEntry }) {
    const st = STATUS[entry.status];
    const restore = async () => {
        try {
            await restoreDeleted(entry.taskId);
            // 成功不提示：行刚从眼前消失、列表那边刚多一行，这本身就是回执。
            // 弹窗是常驻挂着的，再来一条左下角提示只是噪音。
        } catch (e) {
            notify(`撤回失败：${e instanceof Error ? e.message : String(e)}`, "error");
        }
    };
    return (
        <ListRow className="border border-stroke-soft bg-bg-app px-3">
            <div className="flex min-w-0 flex-1 flex-col gap-[3px]">
                <span className="truncate font-mono text-[12px] leading-[18px] font-semibold text-text-1">
                    {truncateMiddle(entry.packFileName, 30)}
                </span>
                <span className="truncate font-mono text-[10px] leading-[14px] text-text-3">
                    {loaderLabel(entry.loader)} {entry.mcVersion} · {ago(entry.deletedAt)}
                    {entry.outputFileName
                        ? ` · ${truncateMiddle(entry.outputFileName, 24)}${
                              entry.outputSizeBytes ? ` ${formatSize(entry.outputSizeBytes)}` : ""
                          }`
                        : ""}
                </span>
            </div>
            <ToneChip tone={st.tone} size="xs" className="shrink-0">
                {st.label}
            </ToneChip>
            <Btn size="xs" className="shrink-0" onClick={() => void restore()}>
                撤回
            </Btn>
        </ListRow>
    );
}
