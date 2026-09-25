/** 应用设置与更新检查 */
import type {
    AppSettings,
    CacheCleanMode,
    CacheUsage,
    CleanReport,
    UpdateChannel,
    UpdateInfo,
    VersionOption,
} from "@/lib/types";
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

/** 项目仓库地址（设置页 GitHub 按钮） */
export const REPO_URL = "https://github.com/banxxx/sideshift";

/**
 * CurseForge Core API Key 的申请入口（第三方应用专用，免费，人工审核后把 Key 发到邮箱）。
 * 官方口径：console.curseforge.com 是「CurseForge for Studios」游戏方控制台，
 * 第三方模组服务走这张表单（docs.curseforge.com/rest-api 与帮助中心
 * 「About the CurseForge API and How to Apply for a Key」同一条链接）。
 * 设置页的「申请 Key」与「从网络添加模组」里的缺 Key 提示共用这一条地址。
 */
export const CURSEFORGE_APPLY_FORM =
    "https://forms.monday.com/forms/dce5ccb7afda9a1c21dab1a1aa1d84eb?r=use1";

/**
 * 最近一份设置（模块级，跨页面挂载存续）：换页会重挂载设置页，没有它就要先画一帧骨架、
 * 再在淡入途中整页换成真内容（`SettingsPage` 的 `if (!settings) return <SettingsSkeleton />`）。
 * `saveSettings` 成功也写这里——不然用户刚改过的那几档，下一次进页会先闪回旧值。
 */
let lastSettings: AppSettings | null = null;

/** 上一份读到/存掉的设置；null = 这个进程还没碰过设置（首帧仍走骨架） */
export function peekSettings(): AppSettings | null {
    return lastSettings;
}

/** 读取设置（Rust: get_settings；mock 走 localStorage） */
export async function getSettings(): Promise<AppSettings> {
    const s = isTauri
        ? await invokeOrMock<AppSettings>("get_settings", undefined, () => mock.mockLoadSettings())
        : await mock.mockLoadSettings();
    lastSettings = s;
    return s;
}

/** 保存设置（Rust: set_settings -> Result；写盘失败会 reject，由设置页外显并回滚） */
export async function saveSettings(s: AppSettings): Promise<void> {
    if (!isTauri) {
        await mock.mockSaveSettings(s);
        lastSettings = s;
        return;
    }
    await invokeOrMock<void>("set_settings", { settings: s }, () =>
        mock.mockSaveSettings(s)
    );
    // 写在 await 之后：后端拒绝时这份必须还是磁盘上那一份（页面紧接着会重读一次）
    lastSettings = s;
}

/** 下载源候选（Rust: list_download_sources） */
export async function listDownloadSources(): Promise<VersionOption[]> {
    if (!isTauri) return mock.mockDownloadSources;
    return invokeOrMock("list_download_sources", undefined, () => mock.mockDownloadSources);
}

/** 打开系统目录选择器（设置页两行目录的「选择」，取消返回 null） */
export async function pickDirectory(): Promise<string | null> {
    if (!isTauri) return "D:\\SideShift\\cache";
    const { open } = await import("@tauri-apps/plugin-dialog");
    const picked = await open({ directory: true, multiple: false });
    return typeof picked === "string" ? picked : null;
}

/**
 * 缓存占用（Rust: cache_usage）。一次真实目录扫描，巨包缓存可能几千条目，
 * 所以不做轮询；调用方（设置页）只在目录换了、手动刷新、清理之后才再读一次，
 * 换页重挂载吃它自己留着的那份结果。
 */
export async function getCacheUsage(): Promise<CacheUsage> {
    if (!isTauri) return mock.mockCacheUsage();
    return invokeOrMock("cache_usage", undefined, () => mock.mockCacheUsage());
}

/**
 * 清理无用文件（Rust: clean_junk）：半截下载 + 孤儿暂存目录 + 空壳目录。
 * 不碰下载缓存本体，所以有任务在跑时也能点。
 */
export async function cleanJunk(): Promise<CleanReport> {
    if (!isTauri) return mock.mockCleanJunk();
    return invokeOrMock("clean_junk", undefined, () => mock.mockCleanJunk());
}

/**
 * 清理下载缓存（Rust: clean_cache）。
 * mode=stale 只删超过 staleDays 未再使用的；mode=all 清空。all 在有任务运行/排队时
 * 由后端 reject（删掉流水线正在取的文件会做出坏包），调用方要把那句话显示出来。
 */
export async function cleanCache(mode: CacheCleanMode): Promise<CleanReport> {
    if (!isTauri) return mock.mockCleanCache(mode);
    return invokeOrMock("clean_cache", { mode }, () => mock.mockCleanCache(mode));
}

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
