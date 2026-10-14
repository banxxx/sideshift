//! 后端→前端的文案出口：把「一句中文」升级成「模板 + 参数」，界面才能翻。
//! - `key` 是带 `{{slot}}` 的简体中文模板，同时就是前端 i18next 的键（「键 = 原文」，见 `src/lib/i18n/index.ts`）；
//!   `zh` 是渲染好的中文整句，供日志、剪贴板、README 与缺目录条目时兜底。
//! - 不做语言判断（前端切语言）；不带参数的固定句子由前端直接拿原文当键。
//!   消费者：`core/verify.rs`（自检明细）、`core/installer.rs`（安装失败的六种明细 `detail_msg`）、
//!   `task_engine`（任务日志行 `TaskLogLine::message_msg`、失败日志行的「标题 · 明细」），
//!   其中外部进程的原文走 `Msg::foreign`。失败卡 `TaskError` 的 title 仍是整句（前端按原文查表），
//!   detail 由 thiserror 拼出：我们自己写的那几种同时带 `detail_msg`，剩下的（带文件名/cause 的诊断串）
//!   由前端按形状归一化（见 `src/lib/log-normalize.ts`）。

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 一句可翻译的后端文案：模板（= 前端 i18n 的键）+ 参数 + 已渲染的中文兜底。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Msg {
    /// 带 `{{slot}}` 的简体中文模板。前端拿它当键查目录
    pub key: String,
    /// 插值参数；`{}` 里没有槽时给 `Value::Null`
    pub args: Value,
    /// 渲染好的中文整句：日志、剪贴板、缺目录条目时的兜底
    pub zh: String,
}

impl Msg {
    /// **外部进程的原文**（本机 Java 安装器逐行输出那一类）：不是本项目写的句子，没有模板可登记，
    /// 界面按原样显示。刻意做成显式构造函数而不是 `From<&str>`——后者会让
    /// `push_log(…, &format!("…"))` 悄悄编译通过，等于给「后端往外发多少句话」那张扫描表开漏。
    pub fn foreign(text: impl Into<String>) -> Self {
        let zh = text.into();
        Self {
            key: zh.clone(),
            args: Value::Null,
            zh,
        }
    }

    /// 把另一句话嵌进本句的槽里（`json!` 收的是可序列化值，这层折一下让调用点读得下去）。
    /// 渲染与界面都认这种形状：见 `render` 的嵌套参数与 `src/lib/log-normalize.ts` 的 `slotText`。
    pub fn arg(&self) -> Value {
        serde_json::to_value(self).unwrap_or_default()
    }

    /// 外部进程的原文（安装器喊的报错、系统 io 消息）当参数嵌进来：界面认不出模板，
    /// 只会按形状归一化之后原样显示那半句，我们自己写的那半句照翻
    pub fn foreign_arg(text: impl Into<String>) -> Value {
        Self::foreign(text).arg()
    }
}

/// 把 `{{slot}}` 换成 `args` 里的值。查不到的槽**原样留着**：
/// 漏传参数要一眼看出来（界面上会露出 `{{count}}`），不能静默变成空串让人以为句子本来就短。
pub fn render(template: &str, args: &Value) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(at) = rest.find("{{") {
        out.push_str(&rest[..at]);
        let Some(end) = rest[at..].find("}}") else {
            // 只有 `{{` 没有闭合：当普通文本处理
            out.push_str(&rest[at..]);
            return out;
        };
        let slot = &rest[at + 2..at + end];
        let value = match args.get(slot) {
            Some(Value::Null) | None => format!("{{{{{slot}}}}}"),
            Some(v) => value_text(v),
        };
        out.push_str(&value);
        rest = &rest[at + end + 2..];
    }
    out.push_str(rest);
    out
}

/// 参数值 → 贴进句子里的那段文本。
///
/// 值本身也可以是一句 `Msg`（`json!({"detail": 某个 Msg})` 嵌进来的对象）：一句日志里套着
/// 另一句带参数的话时（自检汇总那行嵌着失败项的明细、安装收场行嵌着顶层布局），只有把内层
/// 也当模板渲染，界面才能连它一起翻。认的判据是「对象 + `key` 是字符串」，其他对象/数组
/// 仍按紧凑 JSON 贴出（正常用法里不会出现）。
fn value_text(v: &Value) -> String {
    if let Some(template) = v.as_object().and_then(|m| m.get("key")).and_then(|k| k.as_str()) {
        return render(template, v.get("args").unwrap_or(&Value::Null));
    }
    match v.as_str() {
        Some(s) => s.to_string(),
        None => v.to_string(),
    }
}

/// `msg!("解析包清单")` / `msg!("模组 {{count}} 个全部落位", {"count": n})`
///
/// 模板**必须是字面量**：`scripts/i18n.mjs` 靠扫这个字面量算出「后端一共往外发多少句话」，
/// 传变量就扫不到，等于给目录开了个漏网的口子。
#[macro_export]
macro_rules! msg {
    ($template:literal) => {
        $crate::l10n::Msg {
            key: $template.to_string(),
            args: ::serde_json::Value::Null,
            zh: $crate::l10n::render($template, &::serde_json::Value::Null),
        }
    };
    // 参数用 `tt` 收：`{"count": n}` 整体是一个花括号 token 树，按 `expr` 匹配会当成语句块而解析失败
    ($template:literal, $args:tt) => {{
        let args = ::serde_json::json!($args);
        $crate::l10n::Msg {
            key: $template.to_string(),
            zh: $crate::l10n::render($template, &args),
            args,
        }
    }};
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 渲染结果必须与改造前的手写整句逐字一致——不然「中文界面逐字不变」这条从后端就先破了
    #[test]
    fn renders_the_same_sentence_the_caller_used_to_write() {
        let m = msg!("模组 {{count}} 个全部落位", {"count": 12});
        assert_eq!(m.zh, "模组 12 个全部落位");
        assert_eq!(m.key, "模组 {{count}} 个全部落位");
        assert_eq!(m.args, json!({"count": 12}));
    }

    #[test]
    fn slot_order_never_decides_who_is_who() {
        // 正则套句子的死穴在这：两个数字换了顺序就张冠李戴，按槽名取值则不会
        let m = msg!("{{done}}/{{total}} 个已取件", {"total": 146, "done": 41});
        assert_eq!(m.zh, "41/146 个已取件");
    }

    #[test]
    fn missing_slot_stays_visible() {
        assert_eq!(render("缺少 {{name}}", &json!({})), "缺少 {{name}}");
        assert_eq!(render("无槽句", &json!(null)), "无槽句");
        assert_eq!(render("括号不成对 {{", &json!({})), "括号不成对 {{");
        assert_eq!(render("版本 {{v}}", &json!({"v": "1.20.1"})), "版本 1.20.1");
    }

    /// 一句里套一句：外层整句必须与当初手写的那条逐字一致，否则嵌套只是把中文换坏了
    #[test]
    fn nested_msg_renders_through_its_own_template() {
        let inner = msg!("{{done}}/{{total}} 个已取件", {"total": 146, "done": 41});
        let outer = msg!("进度：{{detail}}", {"detail": serde_json::to_value(&inner).unwrap()});
        assert_eq!(outer.zh, "进度：41/146 个已取件");
        // 序列化后仍是对象（前端按 `key` 认出它、连它一起翻）
        assert!(outer.args.get("detail").unwrap().is_object());
    }

    /// 任务存档里存的旧 `TaskLogLine` / `CheckResult` 没有「模板 + 参数」那一项：整条必须还能反序列化，
    /// 否则老任务列表会因为多了一个字段而整块读不出来
    #[test]
    fn msg_round_trips_and_old_payloads_still_load() {
        let m = msg!("构建完成");
        let text = serde_json::to_string(&m).unwrap();
        assert_eq!(serde_json::from_str::<Msg>(&text).unwrap(), m);
        assert_eq!(m.key, "构建完成");
        assert_eq!(m.args, Value::Null);
    }
}
