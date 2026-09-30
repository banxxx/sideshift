/** 系统集成：本地路径拼接与交由系统打开（文件管理器 / 默认程序 / 浏览器） */
import { invoke } from "@tauri-apps/api/core";
import { t } from "@/lib/i18n";
import { notify } from "@/lib/notify";
import { errOf } from "@/lib/errors";
import { isTauri } from "./client";
import { getSettings } from "./settings";

/** 按目录串的分隔符风格拼接路径（仅用于展示与定位，不做规范化） */
export function joinPath(dir: string, fileName: string): string {
    const trimmed = dir.replace(/[\\/]+$/, "");
    return `${trimmed}${trimmed.includes("\\") ? "\\" : "/"}${fileName}`;
}

/** 目录串去掉末尾文件名，得到所在目录（报告页「打开文件夹」） */
export function dirOf(path: string): string {
    return path.replace(/[\\/][^\\/]*$/, "");
}

/** 输出文件完整路径 = 输出目录（任务覆写优先，否则全局设置）+ 文件名 */
export async function resolveOutputPath(
    fileName: string,
    overrideDir?: string
): Promise<string> {
    const { outputDir } = await getSettings();
    return joinPath(overrideDir?.trim() || outputDir, fileName);
}

/** Windows 路径统一成反斜杠。历史脏数据里留过 `C:\Users\you\SideShift/output` 这类
 *  混合写法（旧 settings.json、旧任务记录的 outputPath），交给资源管理器会解析失败。
 *  判据取串里出现过反斜杠——POSIX 路径基本不会有反斜杠，所以 macOS/Linux 上原样返回。
 *  后端命令里也会再归一一次，这里只是让展示与传给系统的串一致。 */
function nativeSlashes(path: string): string {
    return path.includes("\\") ? path.replace(/\//g, "\\") : path;
}

/** 在系统文件管理器中定位文件（浏览器 dev 下为空操作）。
 *  走后端而不是插件的 `revealItemInDir`：JS 侧那条命令受 capability scope 白名单约束，
 *  用户自选的目录枚举不完，见 `commands/system.rs` 的 `open_local_path`。 */
export async function revealPath(path: string): Promise<void> {
    if (!isTauri) return;
    await invoke("reveal_local_path", { path: nativeSlashes(path) });
}

/** 用系统默认程序打开目录（报告页「打开文件夹」/ 列表卡「打开输出目录」） */
export async function openDir(path: string): Promise<void> {
    if (!isTauri) return;
    await invoke("open_local_path", { path: nativeSlashes(path) });
}

/** 在系统浏览器打开外链（设置页 GitHub 按钮） */
export async function openExternal(url: string): Promise<void> {
    if (!isTauri) {
        window.open(url, "_blank", "noopener");
        return;
    }
    try {
        const { openUrl } = await import("@tauri-apps/plugin-opener");
        await openUrl(url);
    } catch (e) {
        // 调用点都是 `void openExternal(...)`，插件拒绝（权限、无关联程序）时界面上一片安静，
        // 只能靠这条提示把真因暴露出来。这里不 rethrow：调用方没有 catch，抛出去仍是未处理拒绝。
        // 原文翻不了，走 errOf 认得的一律给本地化句，认不出的照旧露插件原文
        notify(
            t("lib.couldn-open", "打开链接失败 · {{reason}}", { reason: errOf(e) }),
            "error"
        );
    }
}

/**
 * 项目仓库地址（设置页 GitHub 按钮）。
 * 后端 `check_update` 打的 releases API 是同一个仓库的另一条地址（`commands/settings.rs` 的
 * `UPDATE_URL`），仓库改名/迁移时两处要一起改。
 */
export const REPO_URL = "https://github.com/banxxx/sideshift";
