use serde::{Deserialize, Serialize};

use crate::models::CheckStatus;

/// JDK 探测结果（Rust: probe_java）。转换页在点「开始转换」**之前**就把「本机跑 installer 跑不跑得起来」
/// 显示出来：可预见的失败不该等 30 秒下载走完才说。状态口径沿用 [`CheckStatus`]，
/// 前端那套 Pass/Warn/Fail 的配色与图标不用再分叉一份。
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct JavaProbe {
    pub status: CheckStatus,
    /// 本次**会用上**的那枚 java 的绝对路径（手选命中 = 手选那枚，否则 = 自动挑的）；None = 本机压根没找到
    pub java_path: Option<String>,
    /// 解析出的主版本（8/17/21/25…）；`java -version` 认不出格式时为 None
    pub major: Option<u32>,
    /// 本次转换的最低需求线（由 MC 版本推的那档）；没传需求时 None = 只报有什么、不判够不够
    pub required_major: Option<u32>,
    /// 本机扫到的全部 JDK，按 `JAVA_HOME` → PATH 的顺序：转换页那颗下拉的候选就是它
    pub installed: Vec<JavaInstall>,
    /// 传进来的「手选那枚」已经不在本机了（卸载、换盘符、换机），本次改用自动挑的那一枚
    pub selected_missing: bool,
    /// 一句话结论（带真实数字与落点），失败卡那一路直接显示
    pub detail: String,
}

/// 本机一枚可用（`java -version` 认得出来）的 JDK。转换页的下拉按它列候选，
/// `path` 是标识、`major` 是显示名 —— 同版本两枚时路径不同，只按版本号选不出唯一一枚。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct JavaInstall {
    pub path: String,
    pub major: u32,
}
