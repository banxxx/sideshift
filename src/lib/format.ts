/**
 * 展示格式化工具：文件体积、时钟、耗时、进度条百分比、加载器/输出名口径
 *
 * 这些格式化在 Home/Task/Tasks/Report 多页复用（设计稿中统一为 mono 风格），
 * 抽到公共模块避免各页各写一份导致文案格式漂移。
 */
import type { EnvSource, LoaderKind, PlanMod, SideFlag } from "./types";

/** 84.0 MB / 512 MB / 1.2 GB —— ≥1024 才进阶单位，保留一位小数 */
export function formatSize(bytes: number): string {
    const units = ["B", "KB", "MB", "GB"];
    let v = bytes;
    let i = 0;
    while (v >= 1024 && i < units.length - 1) {
        v /= 1024;
        i++;
    }
    const digits = i === 0 || v >= 100 ? 0 : 1;
    return `${v.toFixed(digits)} ${units[i]}`;
}

/** 本地时钟 HH:MM:SS（日志行时间戳） */
export function formatClock(d: Date | number = new Date()): string {
    const t = new Date(d);
    const p = (n: number) => String(n).padStart(2, "0");
    return `${p(t.getHours())}:${p(t.getMinutes())}:${p(t.getSeconds())}`;
}

/** 耗时：短于 1 分钟显示 "42 秒"，否则 "2分 14秒"（报告页口径） */
export function formatDuration(ms: number): string {
    const s = Math.max(0, Math.round(ms / 1000));
    if (s < 60) return `${s} 秒`;
    return `${Math.floor(s / 60)}分 ${s % 60}秒`;
}

/** 运行中任务的已用时长：MM:SS（任务列表卡右上角） */
export function formatElapsed(ms: number): string {
    const s = Math.max(0, Math.floor(ms / 1000));
    const p = (n: number) => String(n).padStart(2, "0");
    return `${p(Math.floor(s / 60))}:${p(s % 60)}`;
}

/** 0-100 → 0-1 CSS 宽度分数 */
export function toFraction(progress: number): number {
    return Math.min(1, Math.max(0, progress / 100));
}

/** 实时条速率：沿用 formatSize 的单位口径加 /s（1.9 MB/s） */
export function formatRate(bytesPerSec: number): string {
    if (bytesPerSec <= 0) return "—";
    return `${formatSize(bytesPerSec)}/s`;
}

/** 加载器显示名：fabric → Fabric（芯片/下拉/弹窗副标题统一口径） */
export function loaderLabel(loader: LoaderKind): string {
    return { fabric: "Fabric", forge: "Forge", neoforge: "NeoForge" }[loader];
}

/** 端判定依据的直白说法：Convert 卡行与「查看全部」弹窗共用同一口径，别两处各写一份 */
export function evidenceLabel(source: EnvSource = "unknown"): string {
    return {
        mrpack: "整合包声明",
        jarMetadata: "jar 自证",
        modrinthHash: "平台构建",
        modrinthProject: "平台项目",
        nameHeuristic: "名称推断",
        unknown: "无依据",
    }[source];
}

/**
 * 「需人工确认」的行置顶，其余保持原顺序（sort 稳定）：
 * 模组方案卡的预览行与「全部清单」弹窗共用同一口径，两处看到的第一批行必须一致。
 */
export function reviewFirst<T extends { needsReview: boolean }>(rows: T[]): T[] {
    return rows.slice().sort((a, b) => Number(b.needsReview) - Number(a.needsReview));
}

/** 端标签四态：客户端专属 / 服务端专属 / 两端都要 / 判不出来 */
export type SideTag = "client" | "server" | "both" | "review";

const SIDE_TAG_LABEL: Record<SideTag, string> = {
    client: "客户端",
    server: "服务端",
    both: "两端",
    review: "需人工确认",
};

/**
 * 两侧支持度 → 端标签。保留行不能再一概写「服务端」：绝大多数是两端都要，
 * 写成服务端等于谎报它是服务端专属。两个轴都没证据才是「需人工确认」。
 */
export function sideTagOf(m: Pick<PlanMod, "clientSide" | "serverSide">): SideTag {
    const on = (f?: SideFlag) => f === "required" || f === "optional";
    if (on(m.clientSide) && on(m.serverSide)) return "both";
    if (on(m.serverSide)) return "server";
    if (on(m.clientSide)) return "client";
    return "review";
}

/** 端标签文案（三张清单与卡内共用，别各处再抄一份字符串） */
export function sideTagLabel(tag: SideTag): string {
    return SIDE_TAG_LABEL[tag];
}

/** 由整合包文件名推导服务端输出名：xxx.mrpack → xxx-server.zip */
export function outputNameOf(fileName: string): string {
    return `${fileName.replace(/\.(mrpack|zip|7z)$/i, "")}-server.zip`;
}

/**
 * 过长的展示文件名做"掐中间"截断：保头保尾，后缀（.mrpack/.zip…）永远完整。
 * CSS 的 text-overflow 只能尾部省略（会把后缀截丢），故用字符数预算在 JS 层处理；
 * 各调用点按容器宽度传 budget（mono 字体下每字符约等宽，估算可靠）。
 */
export function truncateMiddle(fileName: string, budget = 34): string {
    if (fileName.length <= budget) return fileName;
    const dot = fileName.lastIndexOf(".");
    // 无后缀或点开头的隐藏文件：整名当词干处理
    const hasExt = dot > 0;
    const stem = hasExt ? fileName.slice(0, dot) : fileName;
    const ext = hasExt ? fileName.slice(dot) : "";
    // 预算里刨去省略号与后缀，词干按 6:4 分给头尾
    const keep = budget - 1 - ext.length;
    if (keep < 4) return `${stem.slice(0, Math.max(1, budget - ext.length - 1))}…${ext}`;
    const head = Math.ceil(keep * 0.6);
    const tail = keep - head;
    return `${stem.slice(0, head)}…${stem.slice(stem.length - tail)}${ext}`;
}
