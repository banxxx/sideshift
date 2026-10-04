/**
 * 应用更新那八颗 IPC（查 / 角标 / 记看过 / 取件 / 取消 / 问进度 / 装 / 回读上次结论）+ 两条事件。
 * 谁调它们见 `@/lib/update-store`：判据全在 Rust，这层只做「命令名 ↔ 前端」的映射。
 */
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { EVENTS, type UpdateChannel, type UpdateInfo, type UpdateOutcome, type UpdateStatus } from "@/lib/types";
import * as mock from "@/lib/mock";
import { invokeOrMock, isTauri } from "./client";

/** 应用版本（构建期由 vite 从 package.json 注入，见 vite.config.ts 的 define；前端唯一源） */
export const APP_VERSION = __APP_VERSION__;

/** 版本号去掉预发布位：侧栏左下角的短写法（通道信息在品牌行徽章，那里已经说过一次） */
export const VERSION_CORE = APP_VERSION.split("-", 1)[0];

/**
 * 预发布徽章文案：`1.0.0-beta.2` → "BETA"、`1.1.0-rc.1` → "RC"、纯版本号 → null。
 * 挂在版本号上而不是另立一个配置开关——发稳定版时徽章自己消失，不存在「忘了关」的错配。
 * 只取预发布段开头的字母，所以 `beta.3` / `beta-2` 都归一成 BETA。
 *
 * 它说的是「这枚包造出来时是什么」，跟设置页的更新渠道（订阅哪条更新线）是两条独立信息：
 * 正式版包切到 Beta 之后，这里仍然不出徽章。
 */
export const PRERELEASE_BADGE: string | null = (() => {
    const pre = APP_VERSION.split("-", 2)[1];
    return /^[a-z]+/i.exec(pre ?? "")?.[0].toUpperCase() ?? null;
})();

/**
 * 「跟随当前构建」这条规则的落点：版本号带预发布位就订阅 Beta，纯版本号订阅正式版。
 * Rust 侧 check_update 用的是同一条判据（`!current.pre.is_empty()`），设置页显示的
 * 也就是这个解析结果——所以设置里没选过时，界面说的和后端做的必是同一档。
 */
export const AUTO_UPDATE_CHANNEL: UpdateChannel = APP_VERSION.includes("-") ? "beta" : "stable";

/**
 * 检查更新（Rust: check_update -> UpdateInfo）。
 * 渠道由后端读设置决定，比较也在后端走 semver；前端只消费结论。
 */
export async function checkUpdate(): Promise<UpdateInfo> {
    const fallback = () =>
        mock.mockCheckUpdate(APP_VERSION, mock.mockLoadSettings().updateChannel);
    if (!isTauri) return fallback();
    return invokeOrMock("check_update", undefined, fallback);
}

/**
 * 冷启动那颗角标的初始值（Rust: update_badge -> 该亮时给那一版的结论，否则 null）。
 * 纯读后端那本 24 小时的账、不敲网络：角标要在界面画出来的那一刻就有答案，
 * 等一次网络等于先闪一个空位、亮了再说不出为什么
 */
export function updateBadge(): Promise<UpdateInfo | null> {
    if (!isTauri) return Promise.resolve(mock.mockUpdateBadge());
    return invokeOrMock<UpdateInfo | null>("update_badge", undefined, () => mock.mockUpdateBadge());
}

/**
 * 记一次「弹窗被打开过」（Rust: mark_update_seen）⇒ 角标灭，直到下一趟敲出新版本。
 * 后端那条同步且总是成功，所以这里不 await 也不回 promise：账本坏了最贵的结果只是角标多亮一次
 */
export function markUpdateSeen(): void {
    if (!isTauri) {
        mock.mockMarkUpdateSeen();
        return;
    }
    void invokeOrMock<void>("mark_update_seen", undefined, () => undefined);
}

/**
 * 「跳过这个版本」（Rust: skip_update）⇒ 角标灭，渠道里出现**更新**的那条之前不再亮。
 * 与记看过同一口径：后端同步且总是成功，fire-and-forget；浏览器里没有账本，原样忽略
 */
export function skipUpdate(version: string): void {
    if (!isTauri) return;
    void invokeOrMock<void>("skip_update", { version }, () => undefined);
}

/** 订阅启动后那一趟自动检查的结论（Rust 侧 `update://available`）。浏览器里没有那一趟 ⇒ 订阅是个空动作 */
export function onUpdateAvailable(cb: (info: UpdateInfo) => void): Promise<UnlistenFn> {
    if (!isTauri) return Promise.resolve(() => {});
    return listen<UpdateInfo>(EVENTS.updateAvailable, (ev) => cb(ev.payload));
}

/**
 * 把某一版取到本地并验签（Rust: prepare_update -> UpdateStatus）。
 * 交出去的是 `checkUpdate()` 回的那枚 tag 原文，不是版本号：省掉「前面有没有 v」这一猜。
 * 这一调用会挂到整轮跑完才回（下载与验签都在里面），所以界面**不等它**：
 * 过程走 `onUpdateProgress`，它的返回值只当最后一锤——要么 ready，要么抛种类码。
 * 失败（种类码）也从这里抛——下载没起来与半途断，界面上是同一档「没拿下」。
 */
export async function prepareUpdate(tag: string): Promise<UpdateStatus> {
    const fallback = () => mock.mockPrepareUpdate(tag.replace(/^v/, ""));
    if (!isTauri) return Promise.resolve(fallback());
    return invokeOrMock<UpdateStatus>("prepare_update", { tag }, fallback);
}

/** 取消取件（Rust: cancel_update）。后端那条是同步命令且总是成功，界面照样 await：与其余三颗同形 */
export async function cancelUpdate(): Promise<UpdateStatus> {
    if (!isTauri) return mock.mockCancelUpdate();
    return invokeOrMock<UpdateStatus>("cancel_update", undefined, () => mock.mockCancelUpdate());
}

/** 当前取件状态（Rust: update_status）：弹窗重开、冷启动后靠它对上号，不用自己记 */
export function updateStatus(): Promise<UpdateStatus> {
    if (!isTauri) return Promise.resolve(mock.mockUpdateStatus());
    return invokeOrMock<UpdateStatus>("update_status", undefined, () => mock.mockUpdateStatus());
}

/**
 * 装（Rust: install_update）：静默跑官方安装器换回原目录，然后这个进程就没了。
 *
 * **成功那一路不会回来**——窗口当场消失，所以调用方只该准备失败那一句提示（await + catch 就够）。
 * 成败要等下次启动 `updateOutcome()`：安装器会先杀掉正在运行的我们，那一刻起没有当场回执这种东西。
 */
export async function installUpdate(): Promise<void> {
    if (!isTauri) return mock.mockInstallUpdate();
    return invokeOrMock<void>("install_update", undefined, () => mock.mockInstallUpdate());
}

/**
 * 上一次「立即安装」的结论（Rust: update_outcome）。
 * 后端读一次就把账本收走 ⇒ 这一颗在本进程里**只问一次**，结果留着给所有问它的人：
 * 不缓存的话，挂载两次（StrictMode）就会有一次把账读走、另一次拿到 null，
 * 而那本账正是用户想知道的唯一一件事。
 */
let outcome: Promise<UpdateOutcome | null> | null = null;
export function updateOutcome(): Promise<UpdateOutcome | null> {
    if (!outcome) {
        outcome = isTauri
            ? invokeOrMock<UpdateOutcome | null>("update_outcome", undefined, () =>
                  mock.mockUpdateOutcome()
              )
            : Promise.resolve(mock.mockUpdateOutcome());
    }
    return outcome;
}

/** 订阅取件进度（Rust 侧 `update://progress`，与 `update_status` 同一个载荷） */
export function onUpdateProgress(cb: (s: UpdateStatus) => void): Promise<UnlistenFn> {
    if (!isTauri) return Promise.resolve(mock.onMockUpdateProgress(cb));
    return listen<UpdateStatus>(EVENTS.updateProgress, (ev) => cb(ev.payload));
}
