/**
 * 未接住的异常兜底：没人 catch 的 rejection 与渲染之外的错误统一落到侧栏提示区，界面不再一片安静。
 * `error` 只挂冒泡阶段：资源加载失败（图片 `onerror`）走元素自己的回调，不该在这里播报成故障。
 */
import { t } from "./i18n";
import { errOf } from "./errors";
import { notify } from "./notify";

/** 引擎抛出的内部错误（`TypeError: ...`）翻不了也不该给用户看，界面只给一句通用话 */
export function installErrorGuard(): void {
    window.addEventListener("unhandledrejection", (e) => {
        console.error("[unhandled]", e.reason);
        notify(errOf(e.reason), "error");
    });
    window.addEventListener("error", (e) => {
        console.error("[uncaught]", e.error ?? e.message);
        notify(t("shell.unexpected-error", "界面出了点问题，操作前请先重新载入"), "error");
    });
}
