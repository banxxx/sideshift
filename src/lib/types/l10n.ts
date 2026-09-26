/**
 * 后端文案的线格式（Rust: `crate::l10n::Msg`）。
 *
 * 后端原先只发「渲染好的中文整句」，界面要翻它就两条烂路：整句当键查（带数字的永远查不中）、
 * 或拿正则套（两个数字谁是谁全靠顺序）。所以带参数的句子改成**模板 + 参数**一起发：
 *  `key` 是带 `{{slot}}` 的简体中文模板；前端不拿它直接当 i18next 的键，而是查
 *  `lib/i18n/source-keys.ts` 那张「中文 → 语义键」表换成键（表由 `scripts/i18n.mjs sync` 生成，
 *  换不到就退回 `zh`）—— 后端因此不必知道键名，快照里存的老数据也照样能显示；
 *  `zh` 是后端自己渲染的中文整句，继续当兜底，日志/剪贴板要的也是它。
 *
 * 显示一律走 `backendText(raw, key, args)`：查不到键就退回 `raw`，表现是「那一句掉回中文」。
 */
export interface BackendMsg {
    /** 带 `{{slot}}` 的简体中文模板（前端查表换成语义键） */
    key: string;
    /** 插值参数；无槽的句子为 null */
    args: Record<string, unknown> | null;
    /** 后端渲染好的中文整句 */
    zh: string;
}
