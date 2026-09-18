/**
 * 展示格式化工具：文件体积、时钟、耗时、进度条百分比、加载器/输出名口径
 *
 * 这些格式化在 Home/Task/Tasks/Report 多页复用（设计稿中统一为 mono 风格），
 * 抽到公共模块避免各页各写一份导致文案格式漂移。
 */
import type { LoaderKind } from "./types";

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

/** 加载器显示名：fabric → Fabric（芯片/下拉/弹窗副标题统一口径） */
export function loaderLabel(loader: LoaderKind): string {
    return { fabric: "Fabric", forge: "Forge", neoforge: "NeoForge" }[loader];
}

/** 由整合包文件名推导服务端输出名：xxx.mrpack → xxx-server.zip */
export function outputNameOf(fileName: string): string {
    return `${fileName.replace(/\.(mrpack|zip|7z)$/i, "")}-server.zip`;
}
