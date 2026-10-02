/** 应用版本、更新检查，以及取件（下载 / 验签 / 取消）那四颗 IPC */
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { EVENTS, type UpdateChannel, type UpdateInfo, type UpdateStatus } from "@/lib/types";
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

/** 订阅取件进度（Rust 侧 `update://progress`，与 `update_status` 同一个载荷） */
export function onUpdateProgress(cb: (s: UpdateStatus) => void): Promise<UnlistenFn> {
    if (!isTauri) return Promise.resolve(mock.onMockUpdateProgress(cb));
    return listen<UpdateStatus>(EVENTS.updateProgress, (ev) => cb(ev.payload));
}
