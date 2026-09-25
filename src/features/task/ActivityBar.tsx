/**
 * 「当前动作」在转换进度卡里的两件零件：细子条 + 子行文案。
 *
 * 为什么不替换总进度条而是当它的子条：总条答「整包到哪了」（阶段加权，分钟级），
 * 子条答「手上这个文件到哪了」（秒级）。两者时间尺度不同，合并成一条只会互相失真；
 * 但同一张卡里放两条同类信息可以，靠粗细 + 明度分层读父子关系。
 *
 * 为什么不走日志行：逐块字节进度写进日志环会把日志撑爆
 * （日志量级 = 前端性能预算），后端因此按 150ms 窗口只发 activity 事件。
 */
import { formatRate, formatSize } from "@/lib/format";
import { cn } from "@/lib/utils";
import type { ActivityInfo, ActivityKind } from "@/lib/types";

/**
 * 实时条动词。三种动作三种说法：「下载」是收字节、「打包」是写 zip，
 * 而本机安装是**跑一个外部进程**（分钟级、总量拿不到），不能混叫成下载。
 */
export function activityVerb(kind: ActivityKind): string {
    return kind === "net" ? "下载" : kind === "install" ? "本机安装" : "打包";
}

/** 分母优先字节量；字节未知（响应无 Content-Length）退回条目数，两者都未知按不定态 */
function percentOf(a: ActivityInfo): { percent: number; indeterminate: boolean } {
    if (a.totalBytes > 0) return { percent: (a.doneBytes / a.totalBytes) * 100, indeterminate: false };
    if (a.itemsTotal > 0) return { percent: (a.itemsDone / a.itemsTotal) * 100, indeterminate: false };
    return { percent: 100, indeterminate: true };
}

/**
 * 细子条：紧贴总进度条下方成组。无动作时等高透明占位——
 * 这条槽位常驻，阶段切换与任务收尾都不会让卡高跳一下。
 */
export function ActivitySubBar({ activity }: { activity?: ActivityInfo }) {
    const { percent, indeterminate } = activity ? percentOf(activity) : { percent: 0, indeterminate: false };
    return (
        <div className="h-1 w-full overflow-hidden rounded-full bg-surface-2">
            <div
                className={cn(
                    "h-1 rounded-full transition-[width]",
                    activity ? (indeterminate ? "animate-pulse bg-gold/30" : "bg-gold/50") : "opacity-0"
                )}
                style={{ width: `${percent}%` }}
            />
        </div>
    );
}

/** 子行右侧：重试标记 + 已收/总量 · 速率（无分母时如实写「总量未知」，不假装快满了） */
export function activityMeasure(a: ActivityInfo): string {
    const bytes =
        a.totalBytes > 0
            ? `${formatSize(a.doneBytes)} / ${formatSize(a.totalBytes)}`
            : `${formatSize(a.doneBytes)} · 总量未知`;
    const retry = a.attempt > 1 ? `第 ${a.attempt} 次尝试 · ` : "";
    // 安装器没有总量接口，能如实报的只有「已经装出多少个文件」，字节是写进目录的量而非下载量
    const lead = a.kind === "install" ? `已装出 ${a.itemsDone} 个文件 · ` : "";
    return `${retry}${lead}${bytes} · ${formatRate(a.rateBps)}`;
}
