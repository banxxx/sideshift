use super::*;

/// Aikar's flags：官方推荐的 G1GC 调优参数组（4–12G 内存口径，逐字对照 aikar.co 2020-04-25 版）。
/// `use_aikar_flags` 开启时随 `-Xms`/`-Xmx` 一起拼进 start 脚本
///
/// 按官方原文做的两处自适应（2026-10 核对）：
/// - **大堆档**：内存超 12G 时官方明确要改五个参数（新生代 40/50、区域 16M、预留 15、IHOP 20），
///   原样发 4–12G 口径在大堆上会收到官方说的「老年代回收变多就退回去」的反效果；
/// - **Java 线**：`PerfDisableSharedMem` 是 **JDK 9 才有的选项**，Java 8 上直接
///   `Unrecognized VM option → Could not create the Java Virtual Machine`（服务器拒启，不是性能问题）。
///   老线（≤1.16.5，Java 8）改发 `-XX:-UsePerfData`——Java 8 就有的同目的选项（关掉整个 perf 数据，
///   比只禁 mmap 更激进，但那一档的诉求「别往盘上写 hsperfdata」是同一个）
fn aikar_flags(memory_mb: u32, java_major: u32) -> String {
    let size_tuned = if memory_mb > 12 * 1024 {
        "-XX:G1NewSizePercent=40 -XX:G1MaxNewSizePercent=50 -XX:G1HeapRegionSize=16M          -XX:G1ReservePercent=15 -XX:InitiatingHeapOccupancyPercent=20"
    } else {
        "-XX:G1NewSizePercent=30 -XX:G1MaxNewSizePercent=40 -XX:G1HeapRegionSize=8M          -XX:G1ReservePercent=20 -XX:InitiatingHeapOccupancyPercent=15"
    };
    let perf = if java_major >= 9 {
        "-XX:+PerfDisableSharedMem"
    } else {
        "-XX:-UsePerfData"
    };
    format!(
        "-XX:+UseG1GC -XX:+ParallelRefProcEnabled -XX:MaxGCPauseMillis=200          -XX:+UnlockExperimentalVMOptions -XX:+DisableExplicitGC -XX:+AlwaysPreTouch          {size_tuned} -XX:G1HeapWastePercent=5 -XX:G1MixedGCCountTarget=4          -XX:G1MixedGCLiveThresholdPercent=90 -XX:G1RSetUpdatingPauseTimePercent=5          -XX:SurvivorRatio=32 {perf} -XX:MaxTenuringThreshold=1"
    )
}

/// 内存 + Aikar + 用户附加参数 → 一行 JVM 参数（换行剔除，防注入第二行命令）
pub(super) fn jvm_args(options: &ConversionOptions) -> String {
    let mut parts = vec![format!("-Xmx{}M", options.memory_mb)];
    if options.use_aikar_flags {
        // Aikar 要求 Xms 与 Xmx 相同（行业惯例，也是 AlwaysPreTouch 的生效前提：
        // 只预触到初始堆，不设 -Xms 时 JVM 默认初始堆是物理内存的 1/64，预触利益全失）
        parts.push(format!("-Xms{}M", options.memory_mb));
        let major = options.java_version.trim().parse().unwrap_or(17);
        parts.push(aikar_flags(options.memory_mb, major));
    }
    let extra = options.extra_jvm_args.trim();
    if !extra.is_empty() {
        parts.push(extra.replace(['\r', '\n'], " "));
    }
    parts.join(" ")
}

/// 单行属性值清洗：换行→空格，反斜杠转义（server.properties 的 \n 语义）
pub(super) fn one_line(s: &str) -> String {
    s.replace('\\', "\\\\").replace(['\r', '\n'], " ")
}

/// 同上，再把非 ASCII 一律换成 properties 原生的 `\uXXXX`。
/// 星平面字符（emoji）按 UTF-16 拆成一对代理：`.properties` 只认得 `char` 那一层，
/// 直接写 `\u1F9F1` 会被解成一个非法码点
pub(super) fn one_line_escaped(s: &str) -> String {
    let mut out = String::new();
    for c in one_line(s).chars() {
        if c.is_ascii() {
            out.push(c);
        } else if (c as u32) > 0xFFFF {
            for u in c.encode_utf16(&mut [0u16; 2]) {
                out.push_str(&format!("\\u{u:04X}"));
            }
        } else {
            out.push_str(&format!("\\u{:04X}", c as u32));
        }
    }
    out
}

/// 这个 MC 版本的服务端读属性文件时会不会按 ISO-8859-1 解我们的 UTF-8 字节（会 ⇒ 要转义）。
///
/// 判据只有一条硬事实：**1.20 pre1 起**才是「UTF-8 优先、Latin-1 兜底」，更早一律 Latin-1，
/// 于是一个中文在服务端那边变成三个怪字符。`\uXXXX` 两边都解得对，所以判不出来时选**转**
/// （代价只是文件里那行不可读，反过来判错是真乱码）——快照串 `24w14a` 那一类就走这一格。
/// 老 Beta/Alpha 不是「判不出」而是真读得出线（`b1.7.3` → 7 线），只是同样落在 1.20 之前。
/// 版本线怎么从两套编号（`1.20.1` / `26.3`）里读出来在 `core::mc_version`，与 Java 需求线共用一把尺。
pub(super) fn props_must_escape(mc: &str) -> bool {
    match mc_version::parse(mc) {
        Some(l) => l.line < 20,
        None => true,
    }
}

/// 本机装出来的启动形态（三态，实测三种布局各占一态）
pub(super) enum RunShape {
    /// 没本机安装：包里是 installer jar，start 脚本首次运行先跑 `--installServer`（今日行为）
    FirstBootInstall,
    /// 新式（实测 Forge 1.20.1、NeoForge 26.2）：安装器留下 run 脚本 + `libraries/<...>/{win,unix}_args.txt`。
    /// 我们生成的脚本只引用那两份参数文件，不复用 run 脚本（它用裸 `java`、且 bat 末尾带 `pause`）
    ArgsFiles { win: String, unix: String },
    /// 老 Forge（实测 1.16.5）：没有 run 脚本与参数文件，顶层 universal jar 的 MANIFEST 自带
    /// 相对 `Class-Path: libraries/...` 与 `ServerLaunchArgs` ⇒ 一句 `java -jar <jar> nogui` 就够
    LegacyJar { jar: String },
}

/// 从 installer 自己写的 run 脚本里取参数文件路径。那句固定是
/// `java @user_jvm_args.txt @libraries/net/minecraftforge/forge/1.20.1-47.4.10/win_args.txt %*`，
/// 路径全用 POSIX 斜杠（Windows 侧也认）。不硬编码各家目录布局：Forge 与 NeoForge 的层级不同，
/// 版本号一变又会错，而这两行脚本就是官方给的答案
fn args_token(script: &str, file: &str) -> Option<String> {
    script
        .split_whitespace()
        .map(|t| t.trim_matches(['"', ';']))
        .find(|t| t.starts_with('@') && t.to_ascii_lowercase().ends_with(file))
        // 脚本里的路径要进 zip 条目名与 sh 脚本，统一成正斜杠
        .map(|t| t[1..].replace('\\', "/"))
}

/// 决定启动形态。认不出来的布局直接报错而不是退回首启自装：用户开了这一档要的是
/// 「上传即跑」，悄悄给一份还得联网首装的包比失败更坏
pub(super) fn resolve_shape(installed: Option<&Installed>) -> Result<RunShape, BuilderError> {
    let Some(installed) = installed else {
        return Ok(RunShape::FirstBootInstall);
    };
    let dir = installed.dir.as_path();
    if !installed.report.scripts.is_empty() {
        let mut win: Option<String> = None;
        let mut unix: Option<String> = None;
        for name in ["run.bat", "run.sh"] {
            let Ok(text) = std::fs::read_to_string(dir.join(name)) else {
                continue;
            };
            if win.is_none() {
                win = args_token(&text, "win_args.txt");
            }
            if unix.is_none() {
                unix = args_token(&text, "unix_args.txt");
            }
        }
        // 只认出一家时按同名换后缀推另一家：两份参数文件在同一目录（实测），推完还要落盘验一遍
        let (w, u) = match (win, unix) {
            (Some(w), Some(u)) => (w, u),
            (Some(w), None) => {
                let u = w.replace("win_args.txt", "unix_args.txt");
                (w, u)
            }
            (None, Some(u)) => {
                let w = u.replace("unix_args.txt", "win_args.txt");
                (w, u)
            }
            (None, None) => {
                return Err(BuilderError::Layout(format!(
                    "{} 里的 run 脚本没有引用任何参数文件",
                    dir.display()
                )))
            }
        };
        for p in [&w, &u] {
            if !dir.join(p).is_file() {
                return Err(BuilderError::Layout(format!(
                    "run 脚本指向的 {p} 不在安装目录里"
                )));
            }
        }
        return Ok(RunShape::ArgsFiles { win: w, unix: u });
    }
    // 老式：顶层散 jar 里挑 universal 那枚（`minecraft_server.*.jar` 是官方本体不带启动入口，
    // 安装器 jar 我们不打进包，指过去就是死链）
    if let Some(jar) = installed.report.jars.iter().find(|j| {
        let lower = j.to_ascii_lowercase();
        !lower.starts_with("minecraft_server") && !lower.contains("installer")
    }) {
        return Ok(RunShape::LegacyJar { jar: jar.clone() });
    }
    Err(BuilderError::Layout(format!(
        "{} 顶层既没有 run 脚本也没有可直启的 jar",
        dir.display()
    )))
}

#[cfg(test)]
mod aikar_tests {
    use super::*;

    fn opts(memory: u32, java: &str, aikar: bool) -> ConversionOptions {
        ConversionOptions {
            mc_version: "1.20.1".into(),
            loader_version: "47.4.10".into(),
            java_version: java.into(),
            java_path: String::new(),
            memory_mb: memory,
            generate_scripts: true,
            nogui: false,
            agree_eula: true,
            server_port: 25565,
            motd: String::new(),
            max_players: 20,
            gamemode: "survival".into(),
            difficulty: "easy".into(),
            online_mode: true,
            level_seed: String::new(),
            use_aikar_flags: aikar,
            extra_jvm_args: String::new(),
            output_override: String::new(),
            keep_dirs: Vec::new(),
            keep_files: Vec::new(),
            allow_missing_mods: false,
            install_loader_locally: true,
        }
    }

    /// 关掉开关时只有 -Xmx（不夹带 -Xms 与任何调优参数）
    #[test]
    fn aikar_off_is_just_xmx() {
        let args = jvm_args(&opts(4096, "17", false));
        assert_eq!(args, "-Xmx4096M");
    }

    /// 开启时 Xms == Xmx：官方硬要求，也是 AlwaysPreTouch 只预触到初始堆的生效前提
    #[test]
    fn aikar_on_pins_xms_to_xmx() {
        let args = jvm_args(&opts(6144, "17", true));
        assert!(args.contains("-Xmx6144M"));
        assert!(args.contains("-Xms6144M"));
        let xms = args.find("-Xms").unwrap();
        let xmx = args.find("-Xmx").unwrap();
        assert_eq!(xms, xmx + "-Xmx6144M".len() + 1, "-Xms 应紧跟 -Xmx 之后");
    }

    /// 4-12G 口径（官方基础档）：新生代 30/40、区域 8M、预留 20、IHOP 15
    #[test]
    fn base_tier_matches_aikar_2020_for_common_heaps() {
        let args = jvm_args(&opts(8192, "17", true));
        assert!(args.contains("G1NewSizePercent=30"));
        assert!(args.contains("G1MaxNewSizePercent=40"));
        assert!(args.contains("G1HeapRegionSize=8M"));
        assert!(args.contains("G1ReservePercent=20"));
        assert!(args.contains("InitiatingHeapOccupancyPercent=15"));
        assert!(args.contains("PerfDisableSharedMem"), "JDK9+ 用官方原参数");
        assert!(!args.contains("UsePerfData"));
    }

    /// 超 12G 换官方大堆档：40/50、16M、15、20——官方明说 12G 以上要改这五个
    #[test]
    fn heaps_over_12g_switch_to_the_large_heap_tier() {
        let args = jvm_args(&opts(16384, "21", true));
        assert!(args.contains("G1NewSizePercent=40"));
        assert!(args.contains("G1MaxNewSizePercent=50"));
        assert!(args.contains("G1HeapRegionSize=16M"));
        assert!(args.contains("G1ReservePercent=15"));
        assert!(args.contains("InitiatingHeapOccupancyPercent=20"));
        // 边界：12G 整仍走基础档（官方口径「12GB or less 不动参数」）
        let edge = jvm_args(&opts(12288, "21", true));
        assert!(edge.contains("G1NewSizePercent=30"));
    }

    /// Java 8 线（老 Forge 1.16.5-）没有 PerfDisableSharedMem（JDK 9 才有，
    /// Java 8 上直接「Could not create the Java Virtual Machine」拒启），
    /// 换成 Java 8 就有的同目的选项 -XX:-UsePerfData
    #[test]
    fn java8_line_swaps_in_the_legacy_perf_option() {
        let args = jvm_args(&opts(4096, "8", true));
        assert!(!args.contains("PerfDisableSharedMem"));
        assert!(args.contains("-XX:-UsePerfData"));
        // 其余参数照发：Java 8 全部认得
        assert!(args.contains("UseG1GC") && args.contains("MaxTenuringThreshold=1"));
    }
}
