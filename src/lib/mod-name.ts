/** 模组显示名的取用规矩：只有简体中文档才让内置词典的中文名盖过平台原名。
 *  `name` 本身始终是平台原名（方案与任务存档、端判定证据都吃它），改名只发生在这一层。 */
import { activeLocale } from "@/lib/i18n";

/** 内置词典的中文名优先，其次调用点给的机翻名（「翻译」按钮那份），最后平台原名 */
export function modName(
    m: { name?: string; nameZh?: string } | null | undefined,
    machineZh?: string | null,
): string {
    if (!m) return "";
    if (activeLocale() !== "zh-CN") return m.name ?? "";
    return m.nameZh || machineZh || m.name || "";
}
