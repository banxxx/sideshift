/**
 * 展示格式化工具：文件体积、时钟、耗时、进度条百分比、加载器/输出名口径
 *
 * 这些格式化在 Home/Task/Tasks/Report 多页复用（设计稿中统一为 mono 风格），
 * 抽到公共模块避免各页各写一份导致文案格式漂移。
 */
import type { EnvSource, LoaderKind, PlanMod } from "./types";
import { t } from "@/lib/i18n";

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

/** 完整时刻「2026-09-21 15:32」：任务/报告的起止时间用它。
 *  只给 HH:MM 不够用——任务存档是长期留存的，回看上周的成功记录时「14:32」无法定位是哪天。
 *  日志行时间戳是同一会话内的相对量，仍用 formatClock。 */
export function formatStamp(d: Date | number): string {
    const t = new Date(d);
    const p = (n: number) => String(n).padStart(2, "0");
    return `${t.getFullYear()}-${p(t.getMonth() + 1)}-${p(t.getDate())} ${p(t.getHours())}:${p(t.getMinutes())}`;
}

/** 耗时：短于 1 分钟显示 "42 秒"，否则 "2分 14秒"（报告页口径） */
export function formatDuration(ms: number): string {
    const s = Math.max(0, Math.round(ms / 1000));
    // 单槽的「秒」与「分秒」是两种排版，各给一个键：中文档回落即原文，逐字不变
    if (s < 60) return t("lib.count-sec", "{{count}} 秒", { count: s });
    return t("lib.minutes-seconds", "{{minutes}}分 {{seconds}}秒", {
        minutes: Math.floor(s / 60),
        seconds: s % 60,
    });
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
    // 表建在函数里、每格一条 `t(字面量)`：
    //  - 顶层建表会把字符串冻在首次加载的语言上（切语言不变）
    //  - 表里存键再 `t(表[k])`：第二参没了中文原文，中文档直接露裸键；
    //    `scripts/i18n.mjs check` 也只认字面量调用点，那样写等于漏网
    const label: Record<EnvSource, string> = {
        mrpack: t("lib.modpack-declared", "整合包声明"),
        jarMetadata: t("lib.jar-metadata", "jar 自证"),
        modrinthHash: t("lib.platform-build", "平台构建"),
        modrinthProject: t("lib.platform-project", "平台项目"),
        mirrorProject: t("lib.mirror-project", "镜像项目"),
        nameHeuristic: t("lib.name-guess", "名称推断"),
        unknown: t("lib.basis", "无依据"),
    };
    return label[source];
}

/**
 * 「需人工确认」的行置顶，其余保持原顺序（sort 稳定）：
 * 模组方案卡的预览行与「全部清单」弹窗共用同一口径，两处看到的第一批行必须一致。
 */
export function reviewFirst<T extends { needsReview: boolean }>(rows: T[]): T[] {
    return rows.slice().sort((a, b) => Number(b.needsReview) - Number(a.needsReview));
}

/** 端标签六态（文案一律 5 字）：保留组说服务端，剔除组说客户端，服务端轴没答上才是待确认 */
export type SideTag =
    | "serverRequired"
    | "serverOptional"
    | "clientRequired"
    | "clientOptional"
    | "review"
    | "unknown";

/**
 * 两侧支持度 → 端标签。判据与 `detector::verdict` / 待确认口径同轴，所以标签永远不会与所在页签打架：
 * (必,可) 被判剔 → 说「客户端必装」而不是「服务端可选」；服务端轴没答上的行不替它编结论。
 */
export function sideTagOf(m: Pick<PlanMod, "clientSide" | "serverSide">): SideTag {
    // 两端都没答上（关掉自动剔除时的正常形态、联网反查整批没跑完，以及端证据字段上线前的旧存档）：
    // 那是「没有结论」，不是「有结论但等人过一眼」，不能和 review 共用一枚标签
    if (m.serverSide === undefined && m.clientSide === undefined) return "unknown";
    // 服务端轴没答上（含 (必,无)：会被剔但没有服务端依据）→ 与 detector 的 server_undecided 同判据
    if (m.serverSide === undefined) return "review";
    if (m.serverSide === "required") return "serverRequired";
    // 客户端必需 + 服务端非必需（可选/不支持）→ 裁决是剔除，标签就该说客户端
    if (m.clientSide === "required") return "clientRequired";
    if (m.serverSide === "optional") return "serverOptional";
    // 服务端不支持 → 必剔；这种行只可能属于客户端，客户端那轴没说上也算「可装可不装」
    return "clientOptional";
}

/** 端标签文案（三张清单与卡内共用，别各处再抄一份字符串） */
export function sideTagLabel(tag: SideTag): string {
    const label: Record<SideTag, string> = {
        serverRequired: t("lib.server-required", "服务端必装"),
        serverOptional: t("lib.server-optional", "服务端可选"),
        clientRequired: t("lib.client-required", "客户端必装"),
        clientOptional: t("lib.client-optional", "客户端可选"),
        review: t("lib.needs-review", "需人工确认"),
        unknown: t("lib.unknown", "未判定"),
    };
    return label[tag];
}

/**
 * 两端都必需：这行进了服务端包，玩家的客户端也得装同一个模组，否则连不上或功能缺失。
 * 只用于说明文字（不再做成标签——它不是「这行进不进包」的答案，页签已经答过了）。
 */
export function clientInstallNeeded(m: Pick<PlanMod, "clientSide" | "serverSide">): boolean {
    return m.clientSide === "required" && m.serverSide === "required";
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
