/**
 * 应用更新的跨进程载荷（Rust: `check_update`）。与 `src-tauri/src/models/update.rs` 逐字段对齐。
 * 住在独立文件而不是 settings.ts：更新是有一条状态机的功能域，不只是一张设置行。
 */
import type { UpdateChannel } from "./settings";

/** release 资产的种类（后端按文件名分，认不出的归 `other`，不参与任何安装决策） */
export type UpdateAssetKind = "package" | "signature" | "portable" | "other";

export interface UpdateAsset {
    name: string;
    /** 单位字节，来自 GitHub API 的实测值 */
    size: number;
    kind: UpdateAssetKind;
    /** 宿主不在白名单里时为 false：仍然列出来，但它不是可点的东西 */
    trusted: boolean;
}

/**
 * 不能走「应用内一键更新」的原因（种类码，那句话在前端渲染）。
 * 缺件不许报错、只许退回发布页——否则「签名密钥还没生成」这种过渡期会让用户点一个必失败的按钮。
 */
export type UpdateBlocked =
    | "portable"
    | "missing-key"
    | "no-package"
    | "no-signature"
    | "untrusted-host";

/**
 * 检查更新的结果。有没有更新、能不能应用内更新，全部由后端算完：
 * `1.0.0-beta.2` 与 `1.0.0-beta.10` 这种号，前端拿字符串比一定比反。
 */
export interface UpdateInfo {
    /** 本地版本，与前端注入的 APP_VERSION 同源 */
    current: string;
    /** 该渠道最新的一条 release；仓库还没发过 release 时为 null */
    latest: string | null;
    /**
     * 那条 release 的 tag 原文（`v` 前缀保留）。下载按它重新解析一次 release 再取址：
     * 把 tag 交出去，前端就不必猜「版本号前面有没有 v」，后端也不必猜回来。
     */
    tag: string | null;
    /** latest 严格新于 current 才算有更新 */
    hasUpdate: boolean;
    /** 这一轮实际订阅的渠道（选过用选的那档，没选过用版本号推的那档） */
    channel: UpdateChannel;
    /** 这条 release 的网页址：「打开发布页」的出口，永久保留 */
    releaseUrl: string | null;
    /** 发布时间（GitHub 原样串） */
    publishedAt: string | null;
    /** release 正文：远端文本，直显不翻译 */
    notes: string | null;
    assets: UpdateAsset[];
    /** 产物配齐且宿主可信 ⇒ 允许应用内更新；假 ⇒ 只给「打开发布页」 */
    downloadable: boolean;
    /** downloadable 为假且确有更新可装时的原因；本渠道没有 release 时为 null */
    blocked: UpdateBlocked | null;
}

/**
 * 取件（下载 → 验签 → 待定稿）这一段的生命周期。
 *
 * 与 `UpdateInfo` 分两张类型是刻意的：那张说「有没有新版本」（一次性、来自远端），
 * 这张说「这一版在本地办到哪一步」（会变、可取消、能被清缓存打断）。
 * **没有 `installing` 这一档**：装那一跳的最后一句是退出进程，窗口当场就没了，界面上不存在一段
 * 「安装中」；装成了没有由下次启动的 `UpdateOutcome` 说。
 */
export type UpdateStage = "idle" | "downloading" | "verifying" | "ready" | "failed" | "canceled";

/**
 * 取件状态（Rust: `update_status` 与事件 `update://progress` 共用这一个载荷）。
 * 阶段与进度同一条消息：一次 tick 只发一个对象，前端不必拼两张表才知道「在下第几版」。
 */
export interface UpdateStatus {
    stage: UpdateStage;
    /** 这一轮办的是哪个版本；null = 没有轮次。旧事件晚到一步时靠它对号 */
    version: string | null;
    /** 已收字节（不含验签那一段的读盘） */
    downloaded: number;
    /** 来自 GitHub 的 `asset.size`；0 = 那边没给长度，界面只能说「已下载多少」 */
    total: number;
    /** 失败原因：只有种类码，那句话在 `errors.ts` 一处渲染 */
    error: string | null;
}

/**
 * 上一次「立即安装」的结论（Rust: `update_outcome`）。
 * 只有本机确实试过装一次才有值，之后后端就把那本账收走 ⇒ 冷启动问第二次拿到 null，不会重复提示。
 *
 * 为什么没有「安装器返回的退出码」这种更直接的说法：Windows 上安装器会先把正在运行的我们杀掉，
 * 那一刻起没有任何回调会执行，成败只能靠下次启动比对版本号（判据在 Rust 侧，前端只渲染）。
 */
export type UpdateOutcomeKind = "done" | "unfinished";

export interface UpdateOutcome {
    kind: UpdateOutcomeKind;
    /** 那次试图装上去的版本 */
    attempted: string;
    /** 按下那颗钮时本机是哪一版 */
    previous: string;
}

