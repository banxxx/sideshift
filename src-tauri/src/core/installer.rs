//! 本机执行 loader 官方安装器：把 Forge / NeoForge 装成一个「上传即开服」的目录。
//! - 三家命令形状完全一样（`java -jar <installer>.jar --installServer`，工作目录即目标目录），不按 loader 分叉。
//! - 成功判据用退出码，报因用日志尾巴；退出码 0 但没装出 `libraries/` 仍算失败（安装器是外部程序，末态自己再验一遍）。
//! - stdout 可达数万行，会挤爆任务日志环 ⇒ 只放行少量里程碑行，其余行进一个有界尾巴备查。
//! - 子进程规矩：参数一律 `args([...])` 不进 shell，Windows 带 `CREATE_NO_WINDOW`，`current_dir(dest)`；
//!   取消/超时 ⇒ `kill()`，半成品目录只有本次调用建出来的才删。30 分钟硬超时只是卡死兜底，不是预算。

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use thiserror::Error;

use crate::models::LoaderKind;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 卡死兜底，不是时间预算（本机满速实测 2m53s，见模块头）
pub const INSTALL_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// 轮询步长：取消与超时最晚在此之后生效，同时决定里程碑行落到日志的延迟
const POLL: Duration = Duration::from_millis(150);
/// 扫目录算进度的最小间隔（一次进度事件的成本 = 走一遍 dest 的目录树）
const SCAN_INTERVAL: Duration = Duration::from_millis(500);
/// 失败备查的原始输出尾巴长度
const TAIL_LINES: usize = 60;
/// 报错行最多放行几条：重试多个镜像时同款能刷十几条，够了就别刷日志
const REASON_LINES: usize = 3;
/// 报错信息里带出几行尾巴（栈帧已剔除，剩下基本都是原因）
const TAIL_REPORT_LINES: usize = 8;

pub struct InstallInput<'a> {
    /// 已通过版本校验的那枚 JDK（`core::java::probe` 给的 `java_path`）
    pub java: &'a Path,
    /// 官方 installer jar 的绝对路径（下载阶段已落盘）
    pub installer_jar: &'a Path,
    /// 安装目标目录：安装器拿工作目录当安装位置
    pub dest: &'a Path,
    pub cancel: &'a AtomicBool,
    pub timeout: Duration,
}

/// 安装过程事件。没有百分比：安装器那 22k 行输出不是接口，拿它算进度是瞎猜；
/// 能确定的是「装出了多少文件、多少字节」，走完没走完由上层标「估算」。
pub enum InstallEvent<'a> {
    /// 放行的一条里程碑（固定部分已翻成本地语言；报错行原样带出，只加中文前缀）
    Log(&'a str),
    /// 当前已装出的文件数与字节数
    Progress { files: u64, bytes: u64 },
}

#[derive(Debug)]
pub struct InstallReport {
    pub files: u64,
    pub bytes: u64,
    pub elapsed: Duration,
    /// 顶层 `run.bat` / `run.sh`。老 Forge（实测 1.16.5）装完一个都没有 ⇒ 打包侧据此分叉
    pub scripts: Vec<String>,
    /// 顶层 `*.jar`（老 Forge = universal jar + 那份 `minecraft_server.x.jar`）
    pub jars: Vec<String>,
}

#[derive(Error, Debug)]
pub enum InstallError {
    #[error("安装目录准备失败：{0}")]
    Io(#[from] std::io::Error),
    #[error("启动安装器失败：{0}")]
    Spawn(String),
    #[error("已取消：安装器进程已终止")]
    Cancelled,
    #[error("安装超时（{secs} 秒），安装器进程已终止")]
    Timeout { secs: u64 },
    #[error("安装器退出码 {code}：{tail}")]
    Failed { code: String, tail: String },
    #[error("安装器报告成功，但 {dir} 里没装出 libraries/")]
    Incomplete { dir: String },
}

/// 命令形状。三家实测一样，抽出来单测，免得以后有人按 loader 加分叉。
fn build_command(java: &Path, installer_jar: &Path, dest: &Path) -> Command {
    let mut cmd = Command::new(java);
    cmd.arg("-jar")
        .arg(installer_jar)
        .arg("--installServer")
        .current_dir(dest)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // 不给 -Xmx：JDK 9+ 默认最大堆是物理内存的 1/4，写死 1G 反而把大内存机器压回慢的那条路
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// 一行输出的归类。成败一律看退出码 + 末态目录，这里只决定「值不值得让用户看见」。
#[derive(Debug, PartialEq)]
enum Classified {
    /// 放行：固定文案，认不出来就少说一句，不影响成败
    Milestone(&'static str),
    /// 放行：安装器自己喊的错，原样带出
    Reason(String),
    /// 丢弃：只留在有界尾巴里备查
    Noise,
}

fn classify(line: &str) -> Classified {
    let l = line.trim();
    if l.is_empty() || l.starts_with("at ") || l.starts_with("... ") {
        // 栈帧与「… 12 more」：对用户是噪音，异常行本身已经带了原因
        return Classified::Noise;
    }
    // Forge 写 "minecraft"，NeoForge 写 "Minecraft"
    if l.eq_ignore_ascii_case("Considering minecraft server jar") {
        return Classified::Milestone("下载服务端本体");
    }
    if l.eq_ignore_ascii_case("Downloading libraries") {
        return Classified::Milestone("下载依赖库");
    }
    if l.ends_with("The server installed successfully") {
        return Classified::Milestone("安装器报告成功");
    }
    if l.starts_with("Failed to")
        || l.starts_with("Error:")
        || l.starts_with("Caused by:")
        || l.contains("Exception: ")
    {
        return Classified::Reason(format!("安装器报错：{l}"));
    }
    Classified::Noise
}

/// 有界尾巴：只收非栈帧的原文，超了丢最旧的。取锁失败（另一条读线程正拿着）就跳过这条，
/// 它是备查信息，不值得为它阻塞或报错。
fn push_tail(tail: &Mutex<VecDeque<String>>, line: &str) {
    if matches!(classify(line), Classified::Noise) {
        return;
    }
    if let Ok(mut q) = tail.lock() {
        if q.len() >= TAIL_LINES {
            q.pop_front();
        }
        q.push_back(line.trim().to_string());
    }
}

/// 把尾巴渲染成一行可读的失败原因（日志环按行存，所以不换行拼接）
fn render_tail(tail: &Mutex<VecDeque<String>>) -> String {
    let Ok(q) = tail.lock() else {
        return "（安装器输出未能读取）".to_string();
    };
    let keep: Vec<&String> = q.iter().rev().take(TAIL_REPORT_LINES).collect();
    if keep.is_empty() {
        return "（安装器没有任何输出）".to_string();
    }
    // 倒着取最新的几条，再翻回时间顺序
    let mut parts: Vec<&str> = keep.iter().map(|s| s.as_str()).collect();
    parts.reverse();
    parts.join(" / ")
}

/// 走一遍目录：文件数 + 字节数。排除 `*.log`——安装器会在目标目录里写它自己的
/// `<installer>.jar.log`，它既不是产物，装的途中还一直变大。
fn tally(dir: &Path) -> (u64, u64) {
    let mut files = 0u64;
    let mut bytes = 0u64;
    let mut stack: Vec<PathBuf> = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                stack.push(e.path());
                continue;
            }
            if e.path().extension().is_some_and(|s| s.eq_ignore_ascii_case("log")) {
                continue;
            }
            // 装的途中文件可能被换掉：读不到大小就跳过，别把进度搞成失败
            if let Ok(len) = e.metadata().map(|m| m.len()) {
                files += 1;
                bytes += len;
            }
        }
    }
    (files, bytes)
}

/// 顶层布局探测（第 6 步分叉的依据）：run 脚本与顶层 jar 各列一份
fn top_level(dir: &Path) -> (Vec<String>, Vec<String>) {
    let (mut scripts, mut jars) = (Vec::new(), Vec::new());
    let Ok(entries) = std::fs::read_dir(dir) else {
        return (scripts, jars);
    };
    for e in entries.flatten() {
        if !e.path().is_file() {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_string();
        let lower = name.to_ascii_lowercase();
        if lower == "run.bat" || lower == "run.sh" {
            scripts.push(name);
        } else if lower.ends_with(".jar") {
            jars.push(name);
        }
    }
    (scripts, jars)
}

/// 一条管道的读循环：只把放行的行塞进通道，其余进有界尾巴。
/// 进程一死管道就关闭，这里随之退出（`scope` 的 join 因此不会挂住）。
fn pump<R: Read>(src: R, tail: &Mutex<VecDeque<String>>, tx: &mpsc::Sender<String>, reasons: &AtomicUsize) {
    let mut reader = BufReader::new(src);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            // 读不动就到此为止：剩下的信息在退出码和目录里，不缺这一行
            Err(_) => break,
        }
        match classify(&line) {
            Classified::Milestone(m) => {
                let _ = tx.send(m.to_string());
            }
            Classified::Reason(msg) => {
                if reasons.fetch_add(1, Ordering::Relaxed) < REASON_LINES {
                    let _ = tx.send(msg);
                }
            }
            Classified::Noise => {}
        }
        push_tail(tail, &line);
    }
}

/// 退出状态（进程已停）
enum Stop {
    Finished,
    Errored { code: Option<i32> },
    Cancelled,
    TimedOut,
}

/// 跑一趟安装器。阻塞直到进程结束；`install()` 是唯一入口，故不对外。
fn wait_outcome(
    child: &mut Child,
    input: &InstallInput,
    rx: &mpsc::Receiver<String>,
    on_event: &mut dyn FnMut(InstallEvent),
    started: Instant,
) -> std::io::Result<Stop> {
    let mut last_scan = Instant::now() - SCAN_INTERVAL;
    let mut prev = (0u64, 0u64);
    let stop = loop {
        // 放行的行随到随转（延迟上限就是一个 POLL）
        while let Ok(msg) = rx.try_recv() {
            on_event(InstallEvent::Log(&msg));
        }
        if let Some(status) = child.try_wait()? {
            break if status.success() { Stop::Finished } else { Stop::Errored { code: status.code() } };
        }
        if input.cancel.load(Ordering::Relaxed) {
            // 先杀再退：进程一死两条读线程才拿得到 EOF，scope 的 join 才不会悬着
            let _ = child.kill();
            break Stop::Cancelled;
        }
        if started.elapsed() >= input.timeout {
            let _ = child.kill();
            break Stop::TimedOut;
        }
        if last_scan.elapsed() >= SCAN_INTERVAL {
            last_scan = Instant::now();
            let now = tally(input.dest);
            if now != prev {
                prev = now;
                on_event(InstallEvent::Progress { files: now.0, bytes: now.1 });
            }
        }
        thread::sleep(POLL);
    };
    // 进程已停：通道里最后几条放行行到此都能取到
    while let Ok(msg) = rx.try_recv() {
        on_event(InstallEvent::Log(&msg));
    }
    Ok(stop)
}

/// 执行本机安装：跑一趟安装器，成功则回产物统计。收尾规则见函数内 `cleanup`。
pub fn install(
    input: &InstallInput,
    on_event: &mut dyn FnMut(InstallEvent),
) -> Result<InstallReport, InstallError> {
    let started = Instant::now();
    let created_here = !input.dest.exists();
    std::fs::create_dir_all(input.dest)?;
    // 半成品留着只会让人以为装好了；但只有本次调用建出来的目录才允许删——绝不碰用户已有的目录
    let cleanup = || {
        if created_here {
            let _ = std::fs::remove_dir_all(input.dest);
        }
    };

    let mut child = match build_command(input.java, input.installer_jar, input.dest).spawn() {
        Ok(c) => c,
        // 连进程都没起来（JDK 被删了、路径不对）：刚建的空目录同样不能留
        Err(e) => {
            cleanup();
            return Err(InstallError::Spawn(format!("{}：{e}", input.java.display())));
        }
    };
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let tail: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());
    let (tx, rx) = mpsc::channel::<String>();
    let reasons = AtomicUsize::new(0);

    let stop = {
        let tail = &tail;
        let tx = &tx;
        let reasons = &reasons;
        thread::scope(|s| {
            if let Some(out) = stdout {
                s.spawn(move || pump(out, tail, tx, reasons));
            }
            if let Some(err) = stderr {
                s.spawn(move || pump(err, tail, tx, reasons));
            }
            wait_outcome(&mut child, input, &rx, on_event, started)
        })
    }?;

    match stop {
        Stop::Cancelled => {
            cleanup();
            Err(InstallError::Cancelled)
        }
        Stop::TimedOut => {
            cleanup();
            Err(InstallError::Timeout { secs: input.timeout.as_secs() })
        }
        Stop::Errored { code } => {
            let msg = render_tail(&tail);
            cleanup();
            Err(InstallError::Failed {
                code: code.map(|c| c.to_string()).unwrap_or_else(|| "无（被信号终止）".into()),
                tail: msg,
            })
        }
        Stop::Finished => {
            // 末态自己验：断网那次实测退出码 1 但留下空 libraries/，反过来「码 0 却没装出来」
            // 也不能当成功——安装器是外部程序，别只信它自报的分数。
            if !input.dest.join("libraries").is_dir() {
                cleanup();
                return Err(InstallError::Incomplete {
                    dir: input.dest.display().to_string(),
                });
            }
            let (files, bytes) = tally(input.dest);
            let (scripts, jars) = top_level(input.dest);
            Ok(InstallReport {
                files,
                bytes,
                elapsed: started.elapsed(),
                scripts,
                jars,
            })
        }
    }
}

/// 复用缓存的桶名：`{cache}/installs/{loader}/{mc}-{loader ver}`。字面量在 `core::data_root`
/// （那张表同时是卸载壳的删除清单）；设置页的「Loader 安装」档（`cleanup::clean_installs`）
/// 与这里引的是同一个名字
pub use crate::core::data_root::CACHE_INSTALLS_DIR;
/// 装完才原子改名进真名 ⇒ 复用侧永远看不见半截；这个后缀本身即「上次没走完」的记号
const PARTIAL_SUFFIX: &str = ".partial";
/// 净化后一段键名的长度上限：真版本号不到 20 字符，长得离谱只可能是在灌路径
const SEGMENT_MAX: usize = 64;

/// 版本串会被拼进路径，而它来自整合包声明与用户在版本表里的选择 ⇒ 白名单过滤。
/// 只留字母数字和 `. _ -`，其余（`/`、`\`、`:`、空白、控制字符）一律换成 `-`，
/// 再收掉能逃出去的点段与首尾杂字符。净化不是防恶意，是防一份写着 `../` 的坏 manifest 把缓存目录当跳板。
fn sanitize(raw: &str) -> String {
    let mut out: String = raw
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '-'
            }
        })
        .collect();
    // "...." 一次替换会留下新的 ".."，循环到没有为止（每轮都在变短，必然终止）
    while out.contains("..") {
        out = out.replace("..", "-");
    }
    // Windows 目录名不能以点或空格结尾；开头的 `-` 会被当成参数
    out = out.trim_matches(['.', '-', ' ']).to_string();
    if out.len() > SEGMENT_MAX {
        out.truncate(SEGMENT_MAX);
        out = out.trim_matches(['.', '-', ' ']).to_string();
    }
    if out.is_empty() {
        out = "unknown".to_string();
    }
    out
}

fn segment(loader: LoaderKind) -> &'static str {
    match loader {
        LoaderKind::Fabric => "fabric",
        LoaderKind::Forge => "forge",
        LoaderKind::NeoForge => "neoforge",
    }
}

/// 复用条目 `{cache}/installs/{loader}/{mc}-{loader ver}`。
/// 键里带 mc 版本是给 NeoForge 那种「一个 loader 版本跨多个 MC 版本」留的余地，
/// 也让自检与清理面板能按 MC 版本读。Fabric 走不到这里（它没有独立的 installer jar 可跑，
/// 官方给的那枚 server jar 本身就是安装器 + 启动器，交给服务器首启自装）。
pub fn install_dir(cache_dir: &Path, loader: LoaderKind, mc_version: &str, loader_version: &str) -> PathBuf {
    cache_dir
        .join(CACHE_INSTALLS_DIR)
        .join(segment(loader))
        .join(format!("{}-{}", sanitize(mc_version), sanitize(loader_version)))
}

/// 一个复用条目能不能直接拿来用。判据与 `install()` 的末态判据同一条（有 `libraries/` 且有内容）：
/// 目录树是安装器写完后原子改名进来的，所以「完整」在这里等价于「上一次跑到过末态」。
/// 手工塞进来的空壳、被杀在半路又被人改名的残骸都判不成——顶多浪费一次重装的三分钟。
fn complete_at(dir: &Path) -> Option<InstallReport> {
    if !dir.join("libraries").is_dir() {
        return None;
    }
    let (files, bytes) = tally(dir);
    if files == 0 {
        return None;
    }
    let (scripts, jars) = top_level(dir);
    Some(InstallReport { files, bytes, elapsed: Duration::ZERO, scripts, jars })
}

/// 复用条目的半成品名：`.partial` 后缀本身就等于「上次没走完」的记号
fn partial_path(dir: &Path) -> PathBuf {
    PathBuf::from(format!("{}{PARTIAL_SUFFIX}", dir.display()))
}

pub struct EnsureInput<'a> {
    pub loader: LoaderKind,
    pub mc_version: &'a str,
    pub loader_version: &'a str,
    /// 设置里那颗「复用已装的 Loader」
    pub reuse: bool,
    /// 复用开时的根（`{cache}`，本模块只往 `{cache}/installs` 里写）
    pub cache_dir: &'a Path,
    /// 复用关时的落点：调用方给一个任务私有目录，它跟着任务暂存一起回收
    pub scratch: &'a Path,
    pub java: &'a Path,
    pub installer_jar: &'a Path,
    pub cancel: &'a AtomicBool,
    pub timeout: Duration,
}

#[derive(Debug)]
pub struct Installed {
    /// 装好的 loader 树所在的目录
    pub dir: PathBuf,
    /// true = 命中复用、一次子进程都没起
    pub from_cache: bool,
    pub report: InstallReport,
}

/// 拿到一份可用的 loader 树：命中复用目录就直接用，否则装一份（复用开时经 `.partial` 原子落地）。
/// 与 `install()` 一样阻塞，调用方丢 `spawn_blocking`。
pub fn ensure(
    input: &EnsureInput,
    on_event: &mut dyn FnMut(InstallEvent),
) -> Result<Installed, InstallError> {
    let mut run = |dest: &Path| install(
        &InstallInput {
            java: input.java,
            installer_jar: input.installer_jar,
            dest,
            cancel: input.cancel,
            timeout: input.timeout,
        },
        on_event,
    );

    if !input.reuse {
        let report = run(input.scratch)?;
        return Ok(Installed { dir: input.scratch.to_path_buf(), from_cache: false, report });
    }

    let dest = install_dir(input.cache_dir, input.loader, input.mc_version, input.loader_version);
    if let Some(report) = complete_at(&dest) {
        // 命中也得发一次进度：否则实时条停在 0，看起来像卡住而不是「跳过」
        on_event(InstallEvent::Progress { files: report.files, bytes: report.bytes });
        return Ok(Installed { dir: dest, from_cache: true, report });
    }

    let work = partial_path(&dest);
    // 上次被强杀留下的半成品：`install()` 见目录已存在就不会替我们回收它，先清干净再装
    let _ = std::fs::remove_dir_all(&work);
    let report = run(&work)?;

    // 到这一步 dest 只可能是坏条目（complete_at 刚判过），删掉让位给刚装好的那份。
    // 改名不能跨盘（缓存目录整个换盘得走清理面板），失败就当没装成报出去。
    if dest.exists() {
        std::fs::remove_dir_all(&dest)?;
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::rename(&work, &dest)?;
    Ok(Installed { dir: dest, from_cache: false, report })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 真机日志里的原样取样（三份成功 + 一份断网），验证放行/丢弃的边界
    #[test]
    fn classify_forwards_only_milestones_and_reasons() {
        // 三家共同的成功与阶段行
        assert_eq!(
            classify("Considering minecraft server jar"),
            Classified::Milestone("下载服务端本体")
        );
        assert_eq!(
            classify("Considering Minecraft server jar"),
            Classified::Milestone("下载服务端本体")
        );
        assert_eq!(classify("Downloading libraries"), Classified::Milestone("下载依赖库"));
        assert_eq!(
            classify("The server installed successfully"),
            Classified::Milestone("安装器报告成功")
        );
        // 刷屏大户必须全丢：1.20.1 的 22,343 行里这些占了绝大多数
        for noise in [
            "  Patching net/minecraft/client/main/Main$1 1/1",
            "    Download completed: Checksum validated.",
            "Considering library net.minecraftforge:forge:1.20.1-47.4.10",
            "  Downloading library from https://maven.creeperhost.net/org/ow2/asm/asm/9.8/asm-9.8.jar",
            "Data kindly mirrored by CreeperHost at https://www.creeperhost.net/",
            "JVM info: Microsoft - 25.0.1 - 25.0.1+8-LTS",
            "Host: files.minecraftforge.net [104.21.58.163, 172.67.161.211]",
            "Target Directory: .",
            "Extracted successfully",
            "Using common thread-pool with parallelism 15",
            "  Output: E:\\x\\libraries\\net\\minecraftforge\\forge\\1.16.5-36.2.39\\forge-1.16.5-36.2.39-server.jar Checksum Validated: 9a76786a85d28231a3c171387039b99200a086ba",
            "You can delete this installer file now if you wish",
            "",
            "\tat java.base/sun.nio.ch.Net.pollConnect(Native Method)",
            "\t... 12 more",
        ] {
            assert_eq!(classify(noise), Classified::Noise, "不该放行：{noise}");
        }
    }

    #[test]
    fn classify_catches_the_measured_offline_failure() {
        // 断网那趟（退出码 1）的头两行
        let first = classify("Failed to establish connection to https://files.minecraftforge.net/mirrors-2.0.json");
        assert!(matches!(first, Classified::Reason(_)), "实测失败首行必须放行：{first:?}");
        let second = classify("java.net.ConnectException: Connection refused: getsockopt");
        assert!(matches!(second, Classified::Reason(_)), "异常行必须放行：{second:?}");
        if let Classified::Reason(m) = second {
            assert_eq!(m, "安装器报错：java.net.ConnectException: Connection refused: getsockopt");
        }
    }

    #[test]
    fn tail_keeps_reasons_and_drops_stack_frames() {
        let tail = Mutex::new(VecDeque::new());
        // 断网那趟的真实顺序：原因 + 十几行栈帧 + 收尾
        push_tail(&tail, "Failed to establish connection to https://files.minecraftforge.net/mirrors-2.0.json");
        push_tail(&tail, " Host: files.minecraftforge.net [104.21.58.163]");
        push_tail(&tail, "java.net.ConnectException: Connection refused: getsockopt");
        for i in 0..40 {
            push_tail(&tail, &format!("\tat java.base/sun.nio.ch.Net.pollConnect(Native Method) {i}"));
        }
        push_tail(&tail, "   ");
        let rendered = render_tail(&tail);
        assert!(rendered.contains("Failed to establish connection"), "{rendered}");
        assert!(rendered.contains("ConnectException"), "{rendered}");
        assert!(!rendered.contains("at java.base"), "栈帧不该进报错信息：{rendered}");
        assert!(!rendered.contains('\n'), "报错得是单行：{rendered}");
        // 只留下两行原因：Host 行与 40 行栈帧都不该进备查尾巴
        assert_eq!(tail.lock().unwrap().len(), 2);
    }

    #[test]
    fn tail_is_bounded_and_keeps_the_newest() {
        let tail = Mutex::new(VecDeque::new());
        for i in 0..(TAIL_LINES + 25) {
            push_tail(&tail, &format!("Failed to open #{i}"));
        }
        let q = tail.lock().unwrap();
        assert_eq!(q.len(), TAIL_LINES);
        assert_eq!(q.front().unwrap(), "Failed to open #25");
        assert_eq!(q.back().unwrap(), &format!("Failed to open #{}", TAIL_LINES + 24));
    }

    #[test]
    fn command_shape_is_the_same_for_every_loader() {
        let cmd = build_command(Path::new("C:/jdk/bin/java.exe"), Path::new("C:/cache/forge-installer.jar"), Path::new("D:/dest"));
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        assert_eq!(cmd.get_program(), "C:/jdk/bin/java.exe");
        // 三元素固定：-jar / 安装器绝对路径 / --installServer，没有第四种
        assert_eq!(args.len(), 3);
        assert_eq!(args[0], "-jar");
        assert_eq!(args[1], "C:/cache/forge-installer.jar");
        assert_eq!(args[2], "--installServer");
        assert_eq!(cmd.get_current_dir(), Some(Path::new("D:/dest")));
    }

    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ss-installer-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn mk(dir: &Path, rel: &str, bytes: usize) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, vec![7u8; bytes]).unwrap();
    }

    #[test]
    fn tally_counts_files_but_ignores_install_logs() {
        let dir = std::env::temp_dir().join(format!("ss-installer-tally-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        mk(&dir, "libraries/net/a.jar", 1000);
        mk(&dir, "libraries/net/deep/b.jar", 250);
        mk(&dir, "run.bat", 10);
        // 安装器写在目标目录里的日志不算产物（第 6 步的过滤表同口径）
        mk(&dir, "forge-1.20.1-47.4.10-installer.jar.LOG", 9000);

        let (files, bytes) = tally(&dir);
        assert_eq!((files, bytes), (3, 1260), "{files} / {bytes}");

        let (scripts, jars) = top_level(&dir);
        assert_eq!(scripts, vec!["run.bat".to_string()]);
        assert!(jars.is_empty(), "只有顶层才数 jar：{jars:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn top_level_separates_new_and_old_layout() {
        let dir = std::env::temp_dir().join(format!("ss-installer-top-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // 实测老式（1.16.5）：既无 run 脚本，也只有这两个顶层 jar
        mk(&dir, "forge-1.16.5-36.2.39.jar", 8);
        mk(&dir, "minecraft_server.1.16.5.jar", 8);
        mk(&dir, "libraries/x.jar", 8);
        let (scripts, jars) = top_level(&dir);
        assert!(scripts.is_empty(), "老 Forge 不该报出 run 脚本");
        assert_eq!(jars, vec!["forge-1.16.5-36.2.39.jar".to_string(), "minecraft_server.1.16.5.jar".to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 版本串来自整合包声明与用户在版本表里的选择，会被拼进路径 ⇒ 传什么都不许出 `installs/{loader}/` 这一层
    #[test]
    fn install_dir_cannot_be_steered_out_of_the_bucket() {
        let base = Path::new(if cfg!(windows) { "E:\\cache" } else { "/cache" });
        let bucket = base.join(CACHE_INSTALLS_DIR).join("forge");
        for bad in [
            "../../../../Windows/System32",
            "..\\..\\..\\x",
            "C:\\evil",
            "a/b/c",
            "",
            "   ",
            "....",
            "1.20.1\n../../escape",
            "/absolute/root",
            "%2e%2e%2f",
        ] {
            let p = install_dir(base, LoaderKind::Forge, bad, "47.4.10");
            assert_eq!(p.parent().unwrap(), bucket, "键只能有一层：{p:?}");
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            assert!(!name.is_empty(), "{bad:?} 不能给出空名");
            assert!(!name.contains("..") && !name.contains('/') && !name.contains('\\'), "{bad:?} → {name}");
        }
    }

    #[test]
    fn sanitize_keeps_real_versions_and_neutralizes_the_rest() {
        for real in ["1.20.1", "47.4.10", "26.2.0.88", "1.16.5", "17"] {
            assert_eq!(sanitize(real), real, "正常版本号必须一字不改");
        }
        assert_eq!(sanitize("  1.20.1  "), "1.20.1");
        assert_eq!(sanitize("1.20.1."), "1.20.1", "Windows 不让目录名以点结尾");
        assert_eq!(sanitize(""), "unknown");
        assert_eq!(sanitize("..."), "unknown");
        assert_eq!(sanitize("../../"), "unknown");
        assert_eq!(sanitize("..\\..\\x"), "x");
        assert!(!sanitize("a/../b").contains(".."), "点段必须收掉");
        assert_eq!(sanitize(&"a".repeat(300)).len(), SEGMENT_MAX);
    }

    /// 一次 `ensure` 的公共入参：java 与 installer jar 都可以给不存在的，
    /// 因为这几条测试验的正是「该不该去 spawn」
    fn ensure_input<'a>(
        cache: &'a Path,
        scratch: &'a Path,
        java: &'a Path,
        reuse: bool,
        cancel: &'a AtomicBool,
    ) -> EnsureInput<'a> {
        EnsureInput {
            loader: LoaderKind::Forge,
            mc_version: "1.20.1",
            loader_version: "47.4.10",
            reuse,
            cache_dir: cache,
            scratch,
            java,
            installer_jar: Path::new("no-such-installer.jar"),
            cancel,
            timeout: Duration::from_secs(1),
        }
    }

    const NO_JAVA: &str = "definitely-not-a-java-binary";

    #[test]
    fn reuse_hit_never_starts_a_subprocess() {
        let cache = temp("hit");
        let dest = install_dir(&cache, LoaderKind::Forge, "1.20.1", "47.4.10");
        let scratch = temp("hit-scratch");
        mk(&dest, "libraries/net/x.jar", 1000);
        mk(&dest, "run.bat", 10);
        let cancel = AtomicBool::new(false);
        let mut beats = 0usize;

        let out = ensure(
            &ensure_input(&cache, &scratch, Path::new(NO_JAVA), true, &cancel),
            &mut |ev| {
                if let InstallEvent::Progress { .. } = ev {
                    beats += 1
                }
            },
        )
        .expect("命中的复用条目不该去起进程");

        assert!(out.from_cache);
        assert_eq!(out.dir, dest);
        assert_eq!((out.report.files, out.report.bytes), (2, 1010));
        assert_eq!(out.report.scripts, vec!["run.bat".to_string()]);
        assert_eq!(beats, 1, "命中也得发一次进度，否则实时条停在 0 看着像卡住");
        assert!(!scratch.exists(), "复用开着不该碰 scratch");
        std::fs::remove_dir_all(cache).ok();
    }

    /// 空壳 `libraries/`（断网那次的实测残留形状）+ 上次被杀留下的 `.partial`，两条都验在这里
    #[test]
    fn broken_entry_is_not_a_hit_and_the_stale_partial_is_swept() {
        let cache = temp("broken");
        let input = install_dir(&cache, LoaderKind::Forge, "1.20.1", "47.4.10");
        std::fs::create_dir_all(input.join("libraries")).unwrap();
        let leftover = partial_path(&input);
        mk(&leftover, "half-written.jar", 500);
        let cancel = AtomicBool::new(false);

        let err = ensure(
            &ensure_input(&cache, &temp("broken-scratch"), Path::new(NO_JAVA), true, &cancel),
            &mut |_| {},
        )
        .unwrap_err();

        assert!(matches!(err, InstallError::Spawn(_)), "该走安装器了：{err}");
        assert!(!leftover.exists(), "旧半成品必须先清掉，否则 install() 见目录已存在就不替我们回收");
        assert!(input.join("libraries").is_dir(), "坏条目是既成目录，本次调用没资格顺手删");
        std::fs::remove_dir_all(cache).ok();
    }

    #[test]
    fn scratch_mode_writes_only_where_it_was_told() {
        let cache = temp("scratch-cache");
        let scratch = temp("scratch-dir");
        let cancel = AtomicBool::new(false);

        let err = ensure(
            &ensure_input(&cache, &scratch, Path::new(NO_JAVA), false, &cancel),
            &mut |_| {},
        )
        .unwrap_err();

        assert!(matches!(err, InstallError::Spawn(_)), "{err}");
        assert!(!scratch.exists(), "本次建的空目录不能留下");
        assert!(!cache.join(CACHE_INSTALLS_DIR).exists(), "复用关着就不该碰缓存桶");
        std::fs::remove_dir_all(cache).ok();
    }

    /// 真起一次子进程（本机没有 JDK 就跳过）。给一枚不存在的 installer jar，让 java 当场报错退出：
    /// 验的是这条进程线本身——管道被抽干（不填满就不会卡死）、退出码非 0 走 Failed、
    /// 报错行进了失败信息、本次建的目录被删干净。成功那条路见下面 `#[ignore]` 的真装测试。
    #[test]
    fn install_reports_failure_and_cleans_up_when_the_jar_is_missing() {
        let probe = crate::core::java::probe(&None, &None);
        let Some(found) = probe.java_path else {
            eprintln!("本机没有 java，跳过");
            return;
        };
        let java = PathBuf::from(found);
        let dest = std::env::temp_dir().join(format!("ss-installer-run-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dest);
        let cancel = AtomicBool::new(false);
        let mut lines: Vec<String> = Vec::new();

        let res = install(
            &InstallInput {
                java: &java,
                installer_jar: Path::new("no-such-installer.jar"),
                dest: &dest,
                cancel: &cancel,
                timeout: Duration::from_secs(30),
            },
            &mut |ev| {
                if let InstallEvent::Log(m) = ev {
                    lines.push(m.to_string());
                }
            },
        );

        match res {
            Err(InstallError::Failed { code, tail }) => {
                assert_ne!(code, "0", "java 报的是失败，退出码不能被判成成功");
                assert!(!tail.trim().is_empty(), "失败信息得带上安装器喊的内容");
            }
            Err(other) => panic!("预期 Failed，拿到的是：{other}"),
            Ok(rep) => panic!("jar 不存在却装成功了？产物统计 = {} 个文件", rep.files),
        }
        assert!(!lines.is_empty(), "报错行应放行给用户：{lines:?}");
        assert!(!dest.exists(), "本次调用建出来的半成品目录必须删掉");
    }

    fn var_or(key: &str, default: &str) -> String {
        std::env::var(key).unwrap_or_else(|_| default.to_string())
    }

    /// 真装测试的事件记账（进度只许单调递增，倒退就是扫目录算错了）
    #[derive(Default)]
    struct Stats {
        lines: Vec<String>,
        beats: usize,
        last: (u64, u64),
    }

    impl Stats {
        fn on(&mut self, ev: InstallEvent) {
            match ev {
                InstallEvent::Log(m) => self.lines.push(m.to_string()),
                InstallEvent::Progress { files, bytes } => {
                    assert!(
                        (files, bytes) >= self.last,
                        "进度不能倒退：{:?} → {files}/{bytes}",
                        self.last
                    );
                    self.last = (files, bytes);
                    self.beats += 1;
                }
            }
        }
    }

    /// 真装一趟 + 真复用一趟（联网、约四分钟、临时目录吃 ~160MB），所以默认不跑：
    /// `SS_INSTALLER_JAR=E:/path/forge-1.20.1-47.4.10-installer.jar cargo test --lib installer -- --ignored --nocapture`
    /// 可用 `SS_INSTALLER_MC` / `SS_INSTALLER_VER` / `SS_INSTALLER_LOADER`（forge|neoforge）换目标版本。
    /// 覆盖的是单测碰不到的两件事：真安装器走完 `.partial` → 原子改名落进复用目录；
    /// 第二趟命中复用目录时**一次子进程都不起**（那趟把 java 换成不存在的路径来证明）。
    #[test]
    #[ignore = "联网跑真安装器"]
    fn ensure_a_real_loader_installs_then_hits_the_cache() {
        let jar = std::env::var("SS_INSTALLER_JAR").expect("用 SS_INSTALLER_JAR 指一份本地 installer jar");
        let mc = var_or("SS_INSTALLER_MC", "1.20.1");
        let ver = var_or("SS_INSTALLER_VER", "47.4.10");
        let loader = match var_or("SS_INSTALLER_LOADER", "forge").as_str() {
            "neoforge" => LoaderKind::NeoForge,
            "fabric" => LoaderKind::Fabric,
            _ => LoaderKind::Forge,
        };
        let probe = crate::core::java::probe(&None, &None);
        let java = PathBuf::from(probe.java_path.expect("本机没有 java"));
        let cache = temp("e2e");
        let scratch = temp("e2e-scratch");
        let cancel = AtomicBool::new(false);

        // 用结构体而不是命名闭包收事件：后者会把这些变量一直借到下一次调用结束，两趟之间读不得
        let mut stats = Stats::default();

        let first = ensure(
            &EnsureInput {
                loader,
                mc_version: &mc,
                loader_version: &ver,
                reuse: true,
                cache_dir: &cache,
                scratch: &scratch,
                java: &java,
                installer_jar: Path::new(&jar),
                cancel: &cancel,
                timeout: INSTALL_TIMEOUT,
            },
            &mut |ev| stats.on(ev),
        )
        .unwrap_or_else(|e| panic!("真装失败：{e}"));

        assert!(!first.from_cache);
        assert_eq!(first.dir, install_dir(&cache, loader, &mc, &ver));
        assert!(first.dir.join("libraries").is_dir(), "复用目录里必须有 libraries/");
        assert!(!partial_path(&first.dir).exists(), ".partial 该被改名掉，而不是留在原地");
        assert!(!scratch.exists(), "复用开着就不该碰 scratch");
        assert!(first.report.files > 10, "只装出 {} 个文件，不像装完", first.report.files);
        assert!(stats.beats > 0, "整趟没有一次进度跳动 = 扫目录那条路没走通");
        assert!(
            stats.lines.iter().any(|l| l == "安装器报告成功"),
            "成功尾行应被放行：{:?}",
            stats.lines
        );
        println!(
            "装完：{} 个文件 · {:.1} MB · {:?} · 进度 {} 拍 · 顶层 {:?} + {:?}",
            first.report.files,
            first.report.bytes as f64 / 1048576.0,
            first.report.elapsed,
            stats.beats,
            first.report.scripts,
            first.report.jars,
        );
        println!("放行的行：{:?}", stats.lines);

        // 第二趟：java 换成一枚不存在的 —— 真去 spawn 就会拿到 Spawn 错
        let hit = ensure(
            &EnsureInput {
                loader,
                mc_version: &mc,
                loader_version: &ver,
                reuse: true,
                cache_dir: &cache,
                scratch: &scratch,
                java: Path::new(NO_JAVA),
                installer_jar: Path::new(&jar),
                cancel: &cancel,
                timeout: INSTALL_TIMEOUT,
            },
            &mut |ev| stats.on(ev),
        )
        .unwrap_or_else(|e| panic!("复用命中却去起了进程：{e}"));

        assert!(hit.from_cache);
        assert_eq!(hit.dir, first.dir);
        // 复用侧读出来的统计必须与刚装完那份一致，否则第 6 步的分叉会看走眼
        assert_eq!((hit.report.files, hit.report.bytes), (first.report.files, first.report.bytes));
        assert_eq!(hit.report.scripts, first.report.scripts);
        assert_eq!(hit.report.jars, first.report.jars);
        println!("复用命中：{} 个文件 · 顶层 {:?}", hit.report.files, hit.report.scripts);
        std::fs::remove_dir_all(cache).ok();
    }
}
