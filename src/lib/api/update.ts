/** 应用版本与更新检查 */
import type { UpdateChannel, UpdateInfo } from "@/lib/types";
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
