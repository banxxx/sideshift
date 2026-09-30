/**
 * 后端文案的线格式（Rust: `crate::l10n::Msg`）：带参数的句子「模板 + 参数」一起发。
 * `key` 是带 `{{slot}}` 的简体中文模板，前端查 source-keys 表换成语义键；`zh` 是后端渲染好的中文整句，当兜底、供日志/剪贴板。
 * 显示一律走 `backendText(raw, key, args)`：查不到键退回 raw，表现是那一句掉回中文。
 */
export interface BackendMsg {
    /** 带 `{{slot}}` 的简体中文模板（前端查表换成语义键） */
    key: string;
    /** 插值参数；无槽的句子为 null */
    args: Record<string, unknown> | null;
    /** 后端渲染好的中文整句 */
    zh: string;
}
