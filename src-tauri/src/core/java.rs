//! JDK 定位与版本校验：本机跑 loader installer 之前的前提检查。
//!
//! 三条实测口径决定了这个模块为什么长这样（探针见 `.scratch/installer-probe`，2026-09-25）：
//! - `java -version` 的结果打在 **stderr**（JVM 的历史遗留，不是错误分支），只读 stdout 永远解析不出版本。
//! - 需求线是 MC 的**最低**要求，不是"必须正好这一档"：Java 25 载 1.20.1 Forge 的安装与首启都通，
//!   只剩 `sun.misc.Unsafe` 弃用警告。拿等号当门槛会把高版本用户全挡在门外。
//! - 本机可能同时躺着好几枚 JDK。挑**第一个满足需求的**，`JAVA_HOME` 排在 PATH 之前（那是用户显式指定的），
//!   全不满足才退回第一枚——否则 PATH 上一枚 1.8 会把 JAVA_HOME 里的 17 顶掉，报出一句错的"没装 Java"。
//!
//! 这里只回答"有没有、够不够"，不负责跑安装器（第 3 步的 `core/installer.rs`）。
//! 也不落缓存：结论跟着当前环境走，用户装完 JDK 回来看一眼就该变绿。

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::models::{CheckStatus, JavaProbe};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 候选顺序：`JAVA_HOME/bin/java(.exe)` → PATH 上每一枚 `java(.exe)`（按出现顺序去重）
fn candidates() -> Vec<PathBuf> {
    let exe = if cfg!(windows) { "java.exe" } else { "java" };
    let mut out: Vec<PathBuf> = Vec::new();
    if let Some(home) = std::env::var_os("JAVA_HOME") {
        let p = PathBuf::from(home).join("bin").join(exe);
        if p.is_file() {
            out.push(p);
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let p = dir.join(exe);
            if p.is_file() && !out.contains(&p) {
                out.push(p);
            }
        }
    }
    out
}

/// 跑 `java -version` 拿回文本。拿不到就是拿不到（坏软链、被拦下、或那根本不是 JDK），
/// 上层按"没找到"处理——这里 panic 或报错都没有意义。
fn version_text(path: &Path) -> Option<String> {
    let mut cmd = Command::new(path);
    cmd.arg("-version").stdin(Stdio::null());
    #[cfg(windows)]
    {
        // 探测一次闪一个黑框：转换页每次进页都要跑这趟
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd.output().ok()?;
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    if !stderr.trim().is_empty() {
        return Some(stderr);
    }
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    (!stdout.trim().is_empty()).then_some(stdout)
}

/// 从 `java -version` 文本取主版本。认全部历史格式：
/// `"1.8.0_402"` → 8（`1.` 前缀是 Java 9 之前的老写法）、`"17.0.9"` → 17、`"21"` → 21，
/// 预览位 `"22-ea"` 与 `"17.0.9-preview"` 也认。认不出（非 JDK、输出被改）返回 None。
pub fn parse_major(text: &str) -> Option<u32> {
    let quoted = text.split('"').nth(1)?;
    // 先剥掉 `-ea` / `-preview` / `+35` 这类后缀，再拆 `1.8.0_402` 里的 `_402`
    let head = quoted
        .split(['-', '+', '_'])
        .next()?
        .trim();
    let mut parts = head.split('.').filter(|s| !s.is_empty());
    let first: u32 = parts.next()?.parse().ok()?;
    if first != 1 {
        return Some(first);
    }
    // `1.8` 的主版本在第二段；只写 `1` 的畸形输出按 1 处理
    Some(parts.next().and_then(|s| s.parse().ok()).unwrap_or(1))
}

/// 需求线：`options.java_version` 那串（"8"/"16"/"17"/"21"）。认不出格式 = 没有需求线，只报有什么。
fn required_major(raw: &Option<String>) -> Option<u32> {
    let s = raw.as_deref()?.trim();
    // 容忍 "Java 17" 这种带前缀的值，也容忍 "17.0.9"
    s.split(|c: char| !c.is_ascii_digit())
        .find(|p| !p.is_empty())
        .and_then(|p| p.parse().ok())
}

pub fn probe(required_version: &Option<String>) -> JavaProbe {
    let required = required_major(required_version);

    let mut found: Vec<(PathBuf, u32)> = Vec::new();
    for c in candidates() {
        if let Some(text) = version_text(&c) {
            if let Some(major) = parse_major(&text) {
                found.push((c, major));
            }
        }
    }

    let Some((path, major)) = pick(&found, required) else {
        return JavaProbe {
            status: CheckStatus::Fail,
            java_path: None,
            major: None,
            required_major: required,
            detail: match required {
                Some(r) => format!("未检测到 Java：本次转换需要 Java {r} 及以上"),
                None => "未检测到 Java（本机没有可用的 JDK）".to_string(),
            },
        };
    };

    let enough = required.is_none_or(|r| major >= r);
    let mut detail = match required {
        Some(r) if enough => format!("已检测到 Java {major}（本次需要 Java {r} 及以上）"),
        Some(r) => format!(
            "本机 Java {major} 低于本次需要的 Java {r}，转换会在这里失败"
        ),
        None => format!("已检测到 Java {major}"),
    };
    // 多 JDK 机器上「用的到底是哪一个」必须看得见，但只在真有多枚时说：单枚机器上这行是噪音
    if found.len() > 1 && found.first().map(|(p, _)| p) != Some(&path) {
        detail.push_str(&format!("，用的是 {}", path.display()));
    }

    JavaProbe {
        status: if enough {
            CheckStatus::Pass
        } else {
            CheckStatus::Fail
        },
        java_path: Some(path.display().to_string()),
        major: Some(major),
        required_major: required,
        detail,
    }
}

/// 第一个满足需求的；全不满足取第一枚（宁可报错也别假装"没装"——它装了，只是版本不对）
fn pick(found: &[(PathBuf, u32)], required: Option<u32>) -> Option<(PathBuf, u32)> {
    let hit = found.iter().find(|(_, m)| required.is_none_or(|r| m >= &r));
    hit.or_else(|| found.first()).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_major_reads_every_historical_format() {
        let cases = [
            (r#"openjdk version "1.8.0_402" 2024-01-18"#, 8),
            (r#"java version "1.8.0_202""#, 8),
            (r#"openjdk version "17.0.9" 2023-10-17"#, 17),
            (r#"openjdk version "21" 2023-09-19"#, 21),
            (r#"openjdk version "25.0.1" 2026-01-20"#, 25),
            (r#"openjdk version "22-ea" 2024-03-19"#, 22),
            (r#"openjdk version "17.0.9-preview""#, 17),
            // 微软那枚的完整首行，本机实测输出
            (r#"openjdk version "25.0.1" 2025-10-21 LTS
OpenJDK Runtime Environment Microsoft-12345 (build 25.0.1+8-LTS)
OpenJDK 64-Bit Server VM ..."#, 25),
        ];
        for (text, want) in cases {
            assert_eq!(parse_major(text), Some(want), "输入：{text}");
        }
        // 认不出格式的必须是 None，不能瞎猜一个 1 出来把用户吓住
        assert_eq!(parse_major("A Java Exception has occurred."), None);
        assert_eq!(parse_major(""), None);
    }

    #[test]
    fn required_major_tolerates_prefix_and_minor() {
        let req = |s: Option<&str>| required_major(&s.map(String::from));
        assert_eq!(req(Some("17")), Some(17));
        assert_eq!(req(Some("Java 21")), Some(21));
        assert_eq!(req(Some("17.0.9")), Some(17));
        assert_eq!(req(Some("8")), Some(8));
        assert_eq!(req(None), None);
        assert_eq!(req(Some("")), None);
    }

    #[test]
    fn pick_prefers_candidate_meeting_requirement() {
        let p = |n: u32| PathBuf::from(format!("java{n}"));
        let found = vec![(p(8), 8u32), (p(17), 17), (p(25), 25)];
        // PATH 上第一枚是 1.8 也不能顶掉 JAVA_HOME 里的 17
        assert_eq!(pick(&found, Some(17)), Some((p(17), 17)));
        assert_eq!(pick(&found, Some(21)), Some((p(25), 25)));
        // 全不满足：取第一枚，让上层报"版本不够"而不是"没装"
        assert_eq!(pick(&found, Some(99)), Some((p(8), 8)));
        // 没有需求线：用第一枚
        assert_eq!(pick(&found, None), Some((p(8), 8)));
        assert_eq!(pick(&[], Some(17)), None);
    }

    /// 真机跑一趟（没有候选的机器上直接跳过）。断言只到「至少能认出个主版本」
    #[test]
    fn probe_resolves_major_on_this_machine() {
        let found = candidates();
        if found.is_empty() {
            return;
        }
        let probe = probe(&None);
        assert!(probe.major.is_some(), "候选存在却没解析出版本：{:?}", found);
        assert!(probe.major.unwrap() >= 8);
    }
}
