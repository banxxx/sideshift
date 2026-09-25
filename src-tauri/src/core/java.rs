//! JDK 定位与版本校验：本机跑 loader installer 之前的前提检查。
//!
//! 三条实测口径决定了这个模块为什么长这样（探针见 `.scratch/installer-probe`，2026-09-25）：
//! - `java -version` 的结果打在 **stderr**（JVM 的历史遗留，不是错误分支），只读 stdout 永远解析不出版本。
//! - 需求线是 MC 的**最低**要求，不是"必须正好这一档"：Java 25 载 1.20.1 Forge 的安装与首启都通，
//!   只剩 `sun.misc.Unsafe` 弃用警告。拿等号当门槛会把高版本用户全挡在门外。
//! - 本机可能同时躺着好几枚 JDK。满足需求的几枚里挑 **版本最低** 的那一枚（需求线是"最低要 Java 17"，
//!   不是"请用最新"：拿 25 去跑只要 17 的 Forge 装得成，但那是白捡的行为差异与弃用警告），
//!   同版本号按候选原序（`JAVA_HOME` 排在 PATH 之前，那是用户显式指定的）。
//!   全不满足才退回第一枚——否则 PATH 上一枚 1.8 会把 JAVA_HOME 里的 17 顶掉，报出一句错的"没装 Java"。
//! - **`PATH` 上有一枚，绝不等于本机只有这一枚**（实测这台机器：PATH 与 `JAVA_HOME` 都只指向
//!   MC 启动器解出来的 Java 25，另外六枚躺在 `Program Files\Java`、`Program Files\Microsoft`、`.jdks` 里，
//!   注册表 `JavaSoft\JDK` 只登记了其中 Oracle 那两枚）。所以候选 = 环境里的 + 常见安装根扫出来的，
//!   三处对不上的原因各不相同，只有"逐个跑 `java -version`"这一个动作能同时把三种来源都判干净。
//!
//! 这里回答三件事："本机有哪几枚"（[`installed`]，转换页下拉的候选）、"这次用哪一枚"（自动挑或用户手选，
//! 手选那枚不在了就退回自动）、"够不够这次的需求线"。不负责跑安装器（`core/installer.rs`）。
//! 也不落缓存：结论跟着当前环境走，用户装完 JDK 回来看一眼就该变绿、下拉里就该多出一枚。

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::models::{CheckStatus, JavaInstall, JavaProbe};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 环境里显式可见的那几枚：`JAVA_HOME/bin/java(.exe)` → PATH 上每一枚（按出现顺序去重）
fn env_candidates() -> Vec<PathBuf> {
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

/// 各家安装器的落脚处。**只到厂商这一层，不递归全盘**：全盘扫要摸遍用户目录，
/// 而 JDK 的位置是各家安装程序写死的，列得全比列得深有用。
/// 认不认得是真 JDK，下一步那一趟 `java -version` 说了算，所以这里宁可列宽。
fn root_candidates() -> Vec<PathBuf> {
    let exe = if cfg!(windows) { "java.exe" } else { "java" };
    // (根目录的环境变量, 根目录下的相对段)
    let mut roots: Vec<PathBuf> = Vec::new();
    #[cfg(windows)]
    {
        let vars = ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA", "USERPROFILE"];
        // 厂商目录名：Oracle / Adoptium / Temurin / 微软 / Corretto / Zulu / Liberica 各占一处
        let vendors = [
            "Java",
            "Eclipse Adoptium",
            "Eclipse Foundation",
            "AdoptOpenJDK",
            "Microsoft",
            "Amazon Corretto",
            "Zulu",
            "BellSoft",
        ];
        for var in vars {
            let Some(base) = std::env::var_os(var) else {
                continue;
            };
            for vendor in vendors {
                roots.push(PathBuf::from(&base).join(vendor));
            }
            // IDE 与本机版管理器爱用的那几处用户级目录
            for leaf in [".jdks", "scoop\\apps", ".gradle\\jdks"] {
                roots.push(PathBuf::from(&base).join(leaf));
            }
        }
        // MC 启动器解出来的运行时：这台机器 `JAVA_HOME` 正指着这里，别的启动器（Prism/PCL）也各自备一份
        if let Some(appdata) = std::env::var_os("APPDATA") {
            roots.push(PathBuf::from(appdata).join(".minecraft").join("runtime"));
        }
    }
    #[cfg(not(windows))]
    {
        roots.push(PathBuf::from("/usr/lib/jvm"));
        roots.push(PathBuf::from("/usr/java"));
        roots.push(PathBuf::from("/Library/Java/JavaVirtualMachines"));
        if let Some(home) = std::env::var_os("HOME") {
            let base = PathBuf::from(home);
            roots.push(base.join(".sdkman/candidates/java"));
            roots.push(base.join("/Library/Java/JavaVirtualMachines"));
        }
    }

    let mut out: Vec<PathBuf> = Vec::new();
    for root in roots {
        // 一层厂商目录，再一层 JDK 目录：`{root}/{jdk}/bin/java(.exe)`；
        // macOS 的 `.jdk`  bundle 多两截 `Contents/Home`，同一趟里一并认。
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            let dir = entry.path();
            for tail in [
                PathBuf::from("bin").join(exe),
                PathBuf::from("Contents/Home/bin").join(exe),
                PathBuf::from("current/bin").join(exe),
            ] {
                let p = dir.join(&tail);
                if p.is_file() && !out.contains(&p) {
                    out.push(p);
                }
            }
        }
    }
    out
}

/// 去重用的键：同一个文件可能有多种写法（大小写、`PROGRA~1` 短名、软链），
/// 规范化成同一条才算重复；规范化失败（极少，权限或已消失）就退回原路径，别把候选丢了。
fn dedupe_key(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// 环境显式给的那几枚排前面（`JAVA_HOME` 是用户自己写的），安装根扫出来的按版本新→旧跟在后面。
/// 这一排同时是下拉里的可见顺序；「自动选择」在同版本号的几枚之间按它取先后。
pub fn installed() -> Vec<JavaInstall> {
    let env = env_candidates();
    let seen: std::collections::HashSet<PathBuf> = env.iter().map(|p| dedupe_key(p)).collect();
    let extra: Vec<PathBuf> = root_candidates()
        .into_iter()
        .filter(|p| {
            // Oracle 那枚 `javapath` 是指向真 JDK 的替身，跑得出版本但和正本不是同一个路径，
            // 留着就是同一版本在列表里出现两遍、其中一条还是个看不出归属的替身
            !p.to_string_lossy().to_ascii_lowercase().contains("javapath")
                && !seen.contains(&dedupe_key(p))
        })
        .collect();

    // 一枚 `java -version` 冷启实测约 150ms：串行扫八枚就是一秒多，而这趟在转换页进页就要跑。
    // 真跑不能省（认不出版本的一律不列），能省的只有"一个接一个等"。
    let mut from_env: Vec<JavaInstall> = Vec::new();
    let mut from_roots: Vec<JavaInstall> = Vec::new();
    std::thread::scope(|s| {
        let env_handles: Vec<_> = env
            .iter()
            .map(|p| s.spawn(move || resolve(p)))
            .collect();
        let extra_handles: Vec<_> = extra
            .iter()
            .map(|p| s.spawn(move || resolve(p)))
            .collect();
        from_env = env_handles
            .into_iter()
            .filter_map(|h| h.join().ok().flatten())
            .collect();
        from_roots = extra_handles
            .into_iter()
            .filter_map(|h| h.join().ok().flatten())
            .collect();
    });
    // 只给扫来的那截排序：环境那几枚的顺序是 `JAVA_HOME` → PATH 的原样，sort 会把这条优先级洗掉
    from_roots.sort_by(|a, b| b.major.cmp(&a.major).then_with(|| a.path.cmp(&b.path)));
    from_env.extend(from_roots);
    from_env
}

/// 一个候选 → 一条列表项。认不出版本的不是 JDK（坏软链、替身、被安全软件拦下），不列。
fn resolve(path: &Path) -> Option<JavaInstall> {
    let text = version_text(path)?;
    let major = parse_major(&text)?;
    Some(JavaInstall {
        path: path.display().to_string(),
        major,
    })
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

pub fn probe(required_version: &Option<String>, selected_path: &Option<String>) -> JavaProbe {
    let required = required_major(required_version);
    let found = installed();

    // 手选那枚按路径认（候选本来就是这台机器扫出来的，写法同源）。认不到不当失败处理：
    // 卸载、换盘符、换机都会让快照里那条路径失效，把转换永久钉死比用错一枚更糟，
    // 所以退回自动挑的那一枚，并用 `selected_missing` 让界面说清楚现在用的是谁。
    let wanted = selected_path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let (chosen, selected_missing) = choose(&found, required, wanted);
    // 摘成 owned：下面两处都要把 `found` 整个搬进 `JavaProbe.installed`，
    // 留一个指向它的引用在手里就是自己跟自己借位冲突
    let chosen = chosen.cloned();

    let Some(JavaInstall { path, major }) = chosen else {
        return JavaProbe {
            status: CheckStatus::Fail,
            java_path: None,
            major: None,
            required_major: required,
            installed: found,
            selected_missing,
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
    if found.len() > 1 && found.first().map(|j| j.path.as_str()) != Some(path.as_str()) {
        detail.push_str(&format!("，用的是 {path}"));
    }

    JavaProbe {
        status: if enough {
            CheckStatus::Pass
        } else {
            CheckStatus::Fail
        },
        java_path: Some(path),
        major: Some(major),
        required_major: required,
        installed: found,
        selected_missing,
        detail,
    }
}

/// 这次用哪一枚：手选命中优先，命中不了退回 [`pick`]。第二个返回值 = 手选那枚已不在本机。
/// 单列出来是因为这条判据跨了两边（转换页的事前预览 + installer 那一档的实跑），得是同一个答案。
fn choose<'a>(
    found: &'a [JavaInstall],
    required: Option<u32>,
    wanted: Option<&str>,
) -> (Option<&'a JavaInstall>, bool) {
    let selected = wanted.and_then(|want| found.iter().find(|j| j.path == want));
    (
        selected.or_else(|| pick(found, required)),
        wanted.is_some() && selected.is_none(),
    )
}

/// 满足需求线的几枚里取 **版本最低** 的（`min_by_key` 稳定，同版本号仍是候选原序在前）。
/// 需求线是"这次至少要 Java N"，贴着它跑最贴近 loader 自己测过的那档环境；拿本机最新的 25 去跑
/// 只要 17 的老 Forge 跑得通，但那是白捡的差异，且"自动"更该给出推荐值而不是最大值。
/// 没有需求线（版本串认不出）时无从推荐，尊重环境自己的先后取第一枚。
/// 全不满足仍取第一枚：宁可报"版本不够"，也别假装"没装"——它装了，只是低了。
fn pick(found: &[JavaInstall], required: Option<u32>) -> Option<&JavaInstall> {
    let Some(r) = required else {
        return found.first();
    };
    found
        .iter()
        .filter(|j| j.major >= r)
        .min_by_key(|j| j.major)
        .or_else(|| found.first())
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
        let j = |n: u32| JavaInstall {
            path: format!("java{n}"),
            major: n,
        };
        let found = vec![j(8), j(17), j(25)];
        // PATH 上第一枚是 1.8 也不能顶掉 JAVA_HOME 里的 17
        assert_eq!(pick(&found, Some(17)), Some(&j(17)));
        assert_eq!(pick(&found, Some(21)), Some(&j(25)));
        // 全不满足：取第一枚，让上层报"版本不够"而不是"没装"
        assert_eq!(pick(&found, Some(99)), Some(&j(8)));
        // 没有需求线：无从推荐，用第一枚
        assert_eq!(pick(&found, None), Some(&j(8)));
        assert_eq!(pick(&[], Some(17)), None);
    }

    /// 「自动选择」= 推荐项，不是最新项：需求线 17 时该给 17，哪怕本机躺着 25 和 21
    #[test]
    fn pick_takes_lowest_adequate_not_newest() {
        let j = |p: &str, n: u32| JavaInstall {
            path: p.to_string(),
            major: n,
        };
        let path_of = |r: Option<&JavaInstall>| r.map(|x| x.path.clone());
        // 本机实测的排列：JAVA_HOME 那枚 Java 25 在最前
        let found = vec![j("javaA25", 25), j("javaB17", 17), j("javaC21", 21)];
        assert_eq!(path_of(pick(&found, Some(17))), Some("javaB17".into()));
        assert_eq!(path_of(pick(&found, Some(21))), Some("javaC21".into()));
        assert_eq!(path_of(pick(&found, Some(8))), Some("javaB17".into()));
        // 全不满足仍退回第一枚
        assert_eq!(path_of(pick(&found, Some(99))), Some("javaA25".into()));
        // 同版本号取候选原序在前的那枚（下拉里叫 Java 17(1) 的那一枚）
        let twins = vec![j("javaX17", 17), j("javaY25", 25), j("javaZ17", 17)];
        assert_eq!(path_of(pick(&twins, Some(17))), Some("javaX17".into()));
    }

    /// 手选那枚的三个落点：命中它、没了退回自动、一枚都没有
    #[test]
    fn choose_honours_hand_pick_and_falls_back_when_it_vanishes() {
        let j = |n: u32| JavaInstall {
            path: format!("java{n}"),
            major: n,
        };
        let found = vec![j(8), j(17), j(25)];
        // 手选优先于自动：需求线 17，用户偏要那枚 8 也照他说的来（够不够由 status 说）
        assert_eq!(
            choose(&found, Some(17), Some("java8")),
            (Some(&j(8)), false)
        );
        // 快照里那枚不在本机了：退回自动挑的那一枚，并把这个事实报出去
        assert_eq!(
            choose(&found, Some(17), Some("D:\\gone\\java.exe")),
            (Some(&j(17)), true)
        );
        // 没手选 = 走自动，且不算"失效"
        assert_eq!(choose(&found, Some(17), None), (Some(&j(17)), false));
        assert_eq!(choose(&[], Some(17), Some("java8")), (None, true));
    }

    #[test]
    fn probe_resolves_major_on_this_machine() {
        if env_candidates().is_empty() {
            return;
        }
        let probe = probe(&None, &None);
        assert!(probe.major.is_some(), "候选存在却没解析出版本");
        assert!(probe.major.unwrap() >= 8);
        assert!(!probe.installed.is_empty());
    }

    /// 发现范围只能变宽不能变窄：环境里显式可见的那几枚必须在最终列表里，
    /// 补安装根的那一步（去重、排序、并行收结果）把谁洗掉了都会在这里红。
    #[test]
    fn installed_keeps_every_env_candidate() {
        let env = env_candidates();
        if env.is_empty() {
            return;
        }
        let found = installed();
        for p in &env {
            let want = dedupe_key(p);
            assert!(
                found.iter().any(|j| dedupe_key(Path::new(&j.path)) == want),
                "环境里的 {p:?} 在结果里丢了：{:?}",
                found
            );
        }
    }

    /// 真机诊断（不参与常规跑批）：这台机器到底扫出了哪几枚、整趟多久。
    /// 用户报「我明明装了 X 却没出现在下拉里」时，这一条比读代码快。
    #[test]
    #[ignore]
    fn list_installed_on_this_machine() {
        let started = std::time::Instant::now();
        let found = installed();
        println!(
            "扫出 {} 枚，整趟 {:?}",
            found.len(),
            started.elapsed()
        );
        for j in &found {
            println!("  Java {:>2}  {}", j.major, j.path);
        }
        println!("  env = {}", env_candidates().len());
    }
}
