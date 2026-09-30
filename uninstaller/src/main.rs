//! SideShift 卸载壳：把 NSIS 原生卸载对话框换成与安装壳同一套界面；真正的删除仍交给原生卸载器。
//! 壳自己动手删的目录只有整合包缓存（数据根），因为 NSIS 不知道数据根在哪。
//! 命令行决定身份，三条分支：`--uninstall-child <目录>` → 这份（%TEMP% 副本）跑界面；
//! `/S` `/P` `/UPDATE` `_?=` 任一 → 是外部安装程序/脚本在调，原样转发给原生卸载器并照抄退出码
//! （`_?=` 必须补 `/UPDATE` 同转，否则升级途中的清理会被当成真卸载连用户设置一起端掉）；
//! 其余 → 先把自己复制到 %TEMP% 再重启一次，因为 Windows 删不掉正在运行的 exe。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// 发布产物必须走 Tauri CLI（`pnpm uninstaller`），不能裸 `cargo build --release`：
// 后者会让 tauri-build 给本 crate 打上 `cfg(dev)`，前端不内嵌，窗口开起来是「无法访问页面」
#[cfg(all(dev, not(debug_assertions)))]
compile_error!("卸载壳请用 `pnpm uninstaller` 出包（裸 cargo build --release 会带上 cfg(dev)，产物打不开页面）");

// 目录布局与数据根判定跟主应用/安装壳共用同一份源码（字节相同，不是抄一份）：
// 「卸载时显示的产物目录」和「应用这些年真正在写的目录」必须来自同一条规则
#[allow(dead_code)]
#[path = "../../src-tauri/src/core/data_root.rs"]
mod data_root;

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, WebviewWindowBuilder};

/// 被卸载的主程序文件名（去掉平台后缀）。卸载是否做到，第一条判据就是它没了下来
const MAIN_EXE: &str = "SideShift";
/// 界面上"程序在不在跑"盯的那个进程名。`CheckIfAppIsRunning` 按 exe 名匹配主程序 ⇒ 壳自己的
/// 名字（`SideShift-Uninstall.exe`）不在其列，静默卸载杀进程那一步不会把正在等结果的壳带走
const APP_EXE: &str = "sideshift.exe";
/// 子进程标记。真实参数判定走 `driven_by_installer`，这个标记不能出现在那里
const CHILD: &str = "--uninstall-child";

/// 壳自己的文件名：安装壳投放、注册表登记、卸载钩子删除、这里复制 %TEMP% 副本，四方认同一个常量
const SHELL_EXE: &str = data_root::UNINSTALL_SHELL_NAME;

fn main_exe() -> String {
    format!("{MAIN_EXE}{}", std::env::consts::EXE_SUFFIX)
}

/// NSIS 那份原生卸载器在哪。安装壳装完会把它从安装目录收进配置目录（安装目录里不该躺着两个
/// 卸载入口），但**两处都认**：收失败过一次（目标被占用）或者装着的是这版之前装的安装，
/// 原地那一份就是唯一的出口——只认新位置等于把人留在"找不到卸载入口"那一页
fn nsis_entry(install: &Path) -> Option<PathBuf> {
    [
        install
            .join(data_root::APP_DATA_DIR_NAME)
            .join(data_root::NSIS_UNINSTALLER_STASHED),
        install.join(data_root::NSIS_UNINSTALLER_NAME),
    ]
    .into_iter()
    .find(|p| p.is_file())
}

/// 老布局那两个 identifier 目录（卸载钩子负责删它们）。进度条必须把它们算进分母，
/// 否则会出现"数字一直不涨"；失败文案也要点名它们，不然人不知道还有东西留着
fn legacy_dirs(identifier: &str) -> Vec<PathBuf> {
    ["APPDATA", "LOCALAPPDATA"]
        .into_iter()
        .filter_map(|var| std::env::var_os(var))
        .map(PathBuf::from)
        .map(|dir| dir.join(identifier))
        .collect()
}

/// 开屏快照。`valid` 为假时界面上只有关闭可用（壳被人单独拷出来跑就是这一态）
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    version: String,
    arch: String,
    valid: bool,
    running: bool,
}

/// 卸载完之后界面要复述的两条路径：Rust 侧**卸载之后**实测还在的那两个目录，
/// 不是开屏那份快照（卸载途中盘被拔掉、目录被人手删，这里得跟着变）。
/// `cache_leftover` 只在「该删而没删掉」时才有值，正常卸载它是 None
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Outcome {
    output_dir: Option<String>,
    cache_leftover: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Progress {
    pct: f64,
    done: bool,
}

/// 卸载现场：数据目录里那两个「跟着设置走」的位置。
///
/// 必须在 NSIS 动手**之前**问清：判定要读的 `settings.json` / `installer.json` 就躺在
/// `{install}\appdata` 里，卸载钩子下一步把它端掉，事后再问只会得到"剩余空间最大的预选盘"
/// 那个默认值 —— 人在安装时挑过别处的话那是**另一个**目录，拿它去删缓存就是删错东西
struct Scene {
    /// 转换产物：卸载不碰，完成页要点名它留在哪
    output: PathBuf,
    /// 整合包缓存：这次卸载要一并删掉的那个目录
    cache: PathBuf,
    /// 缓存目录可信到能让壳动手删吗。假 ⇒ 不删，也不在进度里计（宁可留着也不删错）
    deletable: bool,
}

/// 只取缓存与产物两个字段的 settings.json 副本。列成两个字段而不是整个 `AppSettings`：
/// 那结构的其余字段一半没有 `#[serde(default)]`，将来主应用加一个必填字段就会让壳
/// 在这里整份解析失败，而它要的只是这两个路径
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SavedDirs {
    #[serde(default)]
    output_dir: String,
    #[serde(default)]
    cache_dir: String,
}

/// 开屏现场：配置目录里的 settings.json 说了算，读不到的那一项才回落默认布局
fn scene(install: &Path, identifier: &str) -> Option<Scene> {
    let home = home_dir()?;
    let config = config_dir_of(install, identifier);
    let (by_rule, by_rule_cache) =
        data_root::layout_in(&data_root::suggested_root(&home, &config));
    let saved = std::fs::read_to_string(config.join(data_root::SETTINGS_FILE))
        .ok()
        .and_then(|t| serde_json::from_str::<SavedDirs>(&t).ok());
    let (output, cache) = match saved {
        Some(s) => (
            absolute_or(s.output_dir, by_rule),
            absolute_or(s.cache_dir, by_rule_cache),
        ),
        None => (by_rule, by_rule_cache),
    };
    let ok_to_delete = deletable(&cache, &output, &home);
    Some(Scene {
        output,
        cache,
        deletable: ok_to_delete,
    })
}

/// 应用自己的状态在哪：安装版那份在 `{install}\appdata`（`persist::config_dir` 的第 2 档），
/// 装进写不进去的位置时回落到 identifier 目录（第 3 档）。取第一个真放着 settings.json 的；
/// 都没有就是"装完还没启动过应用"，用安装目录那份（installer.json 在那里）
fn config_dir_of(install: &Path, identifier: &str) -> PathBuf {
    let installed = install.join(data_root::APP_DATA_DIR_NAME);
    let settings = |d: &Path| d.join(data_root::SETTINGS_FILE).is_file();
    if settings(&installed) {
        return installed;
    }
    legacy_dirs(identifier)
        .into_iter()
        .find(|d| settings(d))
        .unwrap_or(installed)
}

/// 设置里存过的绝对路径才算数：空串是老版本没写过这个字段，相对路径不知道相对于谁
fn absolute_or(raw: String, fallback: PathBuf) -> PathBuf {
    let p = PathBuf::from(raw.trim());
    if !raw.trim().is_empty() && p.is_absolute() {
        p
    } else {
        fallback
    }
}

/// 这个缓存目录能不能交给壳去 `remove_dir_all`。四条判据全站在"删错的代价"那一侧：
/// 宁可留一份没删干净的缓存，也不能顺着一条看不准的路径把人家的目录端掉
fn deletable(cache: &Path, output: &Path, home: &Path) -> bool {
    cache.is_absolute()
        // 段数不到 3 的是盘根本身或它的直接子级（`E:\`、`C:\Users`）——缓存不可能长这样
        && cache.components().count() >= 3
        // 早先把用户目录当过数据根的那些人，home 里躺着的可能是他全部的文件
        && cache != home
        // 两个目录撞在一处、或产物就建在缓存底下：这一刀不下去，产物优先
        && output != cache
        && !output.starts_with(cache)
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

/// 壳自己删缓存的重试次数。`remove_dir_all` 失败时通常已经删掉了一半（杀软正拿着某个文件），
/// 判据用"目录还在不在"而不是错误码，重试就是接着删剩下那半
const CACHE_TRIES: usize = 3;


/// 卸载对象所在目录（= NSIS 的 `$INSTDIR`）。启动那一刻定死，命令只读它
struct Install(PathBuf);

#[tauri::command]
fn get_snapshot(app: AppHandle, install: tauri::State<'_, Install>) -> Snapshot {
    Snapshot {
        version: app.package_info().version.to_string(),
        arch: arch_label(),
        valid: nsis_entry(&install.0).is_some(),
        running: process_running(APP_EXE),
    }
}

/// 轮询用：用户自己退出应用那一刻「正在运行」那张卡就该消失，不该再多点一下
#[tauri::command]
fn check_running() -> bool {
    process_running(APP_EXE)
}

/// 卸载是纯阻塞 IO（等子进程、反复遍历目录），丢到 blocking 线程池里跑：
/// 卡在 UI 线程上就是整个窗口不响应，这个项目为这件事翻过一次车
#[tauri::command]
async fn run_uninstall(
    app: AppHandle,
    install: tauri::State<'_, Install>,
) -> Result<Outcome, String> {
    let dir = install.0.clone();
    let sink = app.clone();
    tauri::async_runtime::spawn_blocking(move || uninstall(&sink, &dir))
        .await
        .map_err(|e| format!("卸载线程异常退出：{e}"))?
}

/// 打开留在机器上的目录。explorer 的退出码没有意义（成功也返回非 0），所以只 spawn 不收尸
#[tauri::command]
fn open_path(path: String) -> Result<(), String> {
    std::process::Command::new(EXTERNAL_OPENER)
        .arg(&path)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("打开 {path} 失败：{e}"))
}

#[cfg(windows)]
const EXTERNAL_OPENER: &str = "explorer";

#[cfg(not(windows))]
const EXTERNAL_OPENER: &str = "xdg-open";

/// 目录在才报路径，不存在给 None：界面上不放一条指向空气的路径
fn existing(p: &Path) -> Option<String> {
    p.is_dir().then(|| display(p))
}

fn arch_label() -> String {
    match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => other,
    }
    .to_string()
}

/// 路径 → 给用户看的字符串（分隔符按平台归一，前端不再 join）
fn display(p: &Path) -> String {
    p.display().to_string().replace('/', std::path::MAIN_SEPARATOR_STR)
}

fn emit(app: &AppHandle, pct: f64, done: bool) {
    let _ = app.emit(
        "uninstaller://progress",
        Progress { pct: pct.clamp(0.0, 100.0), done },
    );
}

/// 真正的卸载，两段：① 静默跑官方 `uninstall.exe /S` 删程序与老布局；② 壳自己删整合包缓存。
/// 进度按"还剩多少字节"算，成败按残骸判。
///
/// 为什么不看退出码：NSIS 的 `Delete` 失败既不报错也不改退出码（那句 `RMDir "$INSTDIR"` 跑到
/// 的时候 `appdata\` 还在里面，本来就会失败）。照退出码报成功等于把人骗到「已卸载」那一页。
///
/// 为什么缓存这一刀在壳而不在 `hooks.nsh`：数据根是人挑的（安装壳那份预选盘，或设置里自己改的），
/// NSIS 那边只能靠解析 JSON 才知道它在哪，解析错一次就是端掉一个不是我们的目录。
fn uninstall(app: &AppHandle, install: &Path) -> Result<Outcome, String> {
    let entry = nsis_entry(install).ok_or_else(|| {
        format!(
            "{} 里没有 NSIS 的卸载入口，请从原安装目录运行",
            install.display()
        )
    })?;
    let identifier = app.config().identifier.clone();
    let sc = scene(install, &identifier)
        .ok_or_else(|| "读不到本机用户目录，无法确定缓存目录在哪，这一次不动缓存".to_string())?;
    let program = {
        let mut p = vec![install.to_path_buf()];
        p.extend(legacy_dirs(&identifier));
        p
    };
    // 缓存那份字节在动手前量一次并**冻住**：阶段① 每 150ms 重走一遍 GB 级的缓存树，
    // 既抢磁盘又把循环本身拖成"一秒一圈"，而那段时间缓存确实一点没动
    let cache_bytes = if sc.deletable { dir_bytes(&sc.cache) } else { 0 };
    let total = bytes_of(&program) + cache_bytes;
    let started = Instant::now();
    let mut child = std::process::Command::new(&entry)
        .arg("/S")
        .spawn()
        .map_err(|e| format!("启动 {} 失败：{e}", entry.display()))?;

    // ① 程序与老布局
    loop {
        // 主程序没了 + NSIS 卸载器自己也没了（两个位置都空）= 这一段的活做完了
        if !install.join(main_exe()).is_file() && nsis_entry(install).is_none() {
            break;
        }
        emit(app, pct_of(total, bytes_of(&program) + cache_bytes, started), false);
        // 给 20 秒收尸窗口：不带 `_?=` 时 NSIS 会把自己复制到 %TEMP% 再跑，外层那份先退，
        // 此时内层还在删
        if child.try_wait().map_err(|e| format!("等待卸载程序失败：{e}"))?.is_some()
            && started.elapsed() > Duration::from_secs(20)
        {
            return Err(residue(&program));
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    let _ = child.wait();

    // ② 缓存：这一段没有别的进程在配合，删不动就是真被占着，重试几次是给它让路
    if sc.deletable {
        for _ in 0..CACHE_TRIES {
            if !sc.cache.is_dir() {
                break;
            }
            let _ = std::fs::remove_dir_all(&sc.cache);
            emit(
                app,
                pct_of(total, bytes_of(&program) + dir_bytes(&sc.cache), started),
                false,
            );
            std::thread::sleep(Duration::from_millis(300));
        }
    }
    emit(app, 100.0, true);
    Ok(Outcome {
        output_dir: existing(&sc.output),
        cache_leftover: existing(&sc.cache),
    })
}

/// 进度：分子是「已经少掉多少字节」。一个字节都没量到（本来就没东西可删）时只能退化成
/// 时间爬升，封顶 95%——宁可不满也不报假完成
fn pct_of(total: u64, rest: u64, started: Instant) -> f64 {
    if total > 0 {
        (1.0 - rest as f64 / total as f64) * 100.0
    } else {
        95.0 * (1.0 - (-started.elapsed().as_secs_f64() / 3.0).exp())
    }
}

/// 几个目录的字节总和
fn bytes_of(dirs: &[PathBuf]) -> u64 {
    dirs.iter().map(|p| dir_bytes(p)).sum()
}

/// 失败文案：先说"为什么"，再点名还剩什么。没东西可点时也要留一句话，别给一张空卡
fn residue(targets: &[PathBuf]) -> String {
    let left: Vec<String> = targets
        .iter()
        .filter(|p| dir_bytes(p) > 0)
        .map(|p| display(p))
        .collect();
    let base = "有部分文件正被其他程序占用（通常是还开着的 SideShift 窗口或杀毒软件），关掉它再重试";
    if left.is_empty() {
        format!("{base}。")
    } else {
        format!("{base}。还留着：{}", left.join("、"))
    }
}

/// 目录内文件字节总和。卸载过程中它单调下降，所以同时是进度分子和成败判据
fn dir_bytes(dir: &Path) -> u64 {
    let mut stack = vec![dir.to_path_buf()];
    let mut total = 0u64;
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if let Ok(m) = e.metadata() {
                total += m.len();
            }
        }
    }
    total
}

// ---- 启动期那三条身份 ----

/// 我们自己的 exe 所在目录：装好之后它就是 `$INSTDIR`
fn own_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// 这些参数只可能来自"另一个安装程序或脚本"：模板里 `reinst_uninstall` 那段拼的就是它们。
/// 按整个 token 比而不是在整条命令行里找子串——`_?=C:\SP\...` 那种路径里全是巧合
fn driven_by_installer(args: &[String]) -> bool {
    args.iter().any(|a| {
        let up = a.to_ascii_uppercase();
        up == "/S" || up == "/P" || up == "/UPDATE" || up.starts_with("_?=")
    })
}

/// `_?=$INSTDIR` 由父安装包拼出来，卸载器据此"就地卸、不复制到 %TEMP%"。补 `/UPDATE` 是为了
/// 让卸载钩子认出这是升级途中的一次清理。人自己跑的 `/S` 绝不能补——那会让钩子以为还在升级，
/// 设置就永远清不掉了
fn needs_update_flag(args: &[String]) -> bool {
    args.iter().any(|a| a.starts_with("_?="))
        && !args.iter().any(|a| a.eq_ignore_ascii_case("/UPDATE"))
}

/// 从原始命令行里剥掉第一个 token（我们自己的 exe 路径），剩下的**原样**交给 `uninstall.exe`。
///
/// 必须用原始命令行而不是 `env::args()` 重拼：模板那句 `StrCpy $R1 "$R1 _?=$4"` 没给路径加引号，
/// 装进 `C:\Program Files\...` 时它在我们的 argv 里已经断成两截，重拼只会拼出一个错目录
fn strip_first_token(line: &str) -> &str {
    let t = line.trim_start();
    match t.strip_prefix('"') {
        Some(after) => match after.find('"') {
            Some(i) => after[i + 1..].trim_start(),
            None => "",
        },
        None => match t.find(char::is_whitespace) {
            Some(i) => t[i..].trim_start(),
            None => "",
        },
    }
}

#[cfg(windows)]
fn raw_command_line() -> String {
    // 这条函数在 windows-sys 里挂在 `System::Environment`，不在直觉上的那个 Console 栏下
    use windows_sys::Win32::System::Environment::GetCommandLineW;
    unsafe {
        let p = GetCommandLineW();
        let mut len = 0usize;
        while *p.add(len) != 0 {
            len += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(p as *const u16, len))
    }
}

#[cfg(not(windows))]
fn raw_command_line() -> String {
    std::env::args().skip(1).collect::<Vec<_>>().join(" ")
}

/// 转发：参数按原样贴上去（连原来的引号和空格都不改），所以 Windows 走 `raw_arg`
#[cfg(windows)]
fn forward(entry: &Path, rest: &str, extra: &str) -> std::io::Result<std::process::ExitStatus> {
    use std::os::windows::process::CommandExt;
    let mut cmd = std::process::Command::new(entry);
    if !rest.is_empty() {
        cmd.raw_arg(rest);
    }
    if !extra.is_empty() {
        cmd.raw_arg(extra);
    }
    cmd.status()
}

#[cfg(not(windows))]
fn forward(entry: &Path, rest: &str, extra: &str) -> std::io::Result<std::process::ExitStatus> {
    let mut cmd = std::process::Command::new(entry);
    for a in rest.split_whitespace().chain(extra.split_whitespace()) {
        cmd.arg(a);
    }
    cmd.status()
}

/// dev 预览口：`SIDESHIFT_UNINSTALL_DIR` 指一个真装着 SideShift 的目录。不带这条，dev 那份 exe
/// 旁边只有 `target/debug`，界面永远停在「找不到安装位置」，改一处样式要看两遍错误卡。
/// 发布构建不读它（返回 None），所以它没有能力影响真实卸载
#[cfg(debug_assertions)]
fn dev_dir() -> Option<PathBuf> {
    std::env::var_os("SIDESHIFT_UNINSTALL_DIR").map(PathBuf::from)
}

#[cfg(not(debug_assertions))]
fn dev_dir() -> Option<PathBuf> {
    None
}

/// 把自己复制到 %TEMP% 再启动一份，父进程立刻退出。
/// 复制而不是"原地跑完再自删"：正在运行的 exe 删不掉，而这次卸载的目标目录就是它所在的那个
fn relaunch_from_temp(install: &Path) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("读自身路径失败：{e}"))?;
    let dir = std::env::temp_dir().join(format!("sideshift-uninstall-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("建临时目录 {} 失败：{e}", dir.display()))?;
    let copy = dir.join(SHELL_EXE);
    std::fs::copy(&exe, &copy).map_err(|e| format!("复制到 {} 失败：{e}", copy.display()))?;
    std::process::Command::new(&copy)
        .arg(CHILD)
        .arg(display(install))
        .spawn()
        .map_err(|e| format!("启动卸载界面失败：{e}"))?;
    Ok(())
}

fn run_ui(install: PathBuf) {
    // WebView2 的 profile 必须放 %TEMP%，不能要默认值：Tauri 在 data_directory 为 None 时会强塞
    // `%LOCALAPPDATA%\{identifier}`，而那正是本次卸载要清掉的老布局目录之一——一个正在跑自己的
    // 卸载器，不该把要删的那个目录锁住
    let webview = std::env::temp_dir().join(format!("sideshift-uninstall-view-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&webview);

    tauri::Builder::default()
        .manage(Install(install))
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            check_running,
            run_uninstall,
            open_path
        ])
        .setup(move |app| {
            // 窗口选项的单源仍是 tauri.conf.json（那里 `"create": false`）：绝对路径只能在建窗时
            // 给，所以这里从配置建窗、只补 data_directory 这一项，不抄第二份尺寸表
            let Some(cfg) = app.config().app.windows.first().cloned() else {
                return Ok(());
            };
            let mut window = WebviewWindowBuilder::from_config(app.handle(), &cfg)?;
            window = window.data_directory(webview.clone());
            window.build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("SideShift 卸载壳启动失败");
}

/// 本机有没有在跑 `SideShift.exe`。只报事实，不动它：静默卸载那一步 NSIS 自己会按 exe 名结束进程，
/// 壳再补一次 TerminateProcess 只是把同一个动作提前，还把"转换写了一半"这个责任揽到了壳身上
#[cfg(windows)]
fn process_running(name: &str) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snap == INVALID_HANDLE_VALUE {
        return false;
    }
    // windows-sys 不给这些结构体 impl Default，只能 zeroed；dwSize 不填 First 必失败
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
    let mut found = false;
    let mut ok = unsafe { Process32FirstW(snap, &mut entry) };
    while ok != 0 {
        if utf16_until_nul(&entry.szExeFile).eq_ignore_ascii_case(name) {
            found = true;
            break;
        }
        ok = unsafe { Process32NextW(snap, &mut entry) };
    }
    unsafe { CloseHandle(snap) };
    found
}

#[cfg(windows)]
fn utf16_until_nul(buf: &[u16]) -> String {
    let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// 非 Windows 只用于跑测试/看界面：没有安装目录可查，一律当作没在跑
#[cfg(not(windows))]
fn process_running(_name: &str) -> bool {
    false
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // ① 被复制到 %TEMP% 的那一份：只有它跑界面
    if let Some(i) = args.iter().position(|a| a == CHILD) {
        let dir = args
            .get(i + 1)
            .map(|p| PathBuf::from(p.trim_matches('"')))
            .unwrap_or_else(own_dir);
        run_ui(dir);
        return;
    }

    // ② 另一个安装程序/脚本在调我们：不弹界面，原样转发
    if driven_by_installer(&args) {
        if let Some(entry) = nsis_entry(&own_dir()) {
            let extra = if needs_update_flag(&args) { " /UPDATE" } else { "" };
            let line = raw_command_line();
            let rest = strip_first_token(&line);
            if let Ok(status) = forward(&entry, rest, extra) {
                std::process::exit(status.code().unwrap_or(-1));
            }
        }
        // 转发失败（占用/权限）或压根没有入口时落回下面的界面：那句"找不到安装位置"
        // 至少要说给人听
    }

    // ③ 人自己打开的
    let dir = dev_dir().unwrap_or_else(own_dir);
    let installed = nsis_entry(&dir).is_some();
    if installed && !cfg!(debug_assertions) {
        if let Err(e) = relaunch_from_temp(&dir) {
            // 复制失败也要就地给个界面：卸载会删不掉自己那一份，但"进度条卡住 + 一张写明原因的卡"
            // 仍然比双击了没反应强
            eprintln!("{e}");
        } else {
            return;
        }
    }
    run_ui(dir);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_installer_args_count_as_driven() {
        assert!(driven_by_installer(&["/S".into()]));
        assert!(driven_by_installer(&["_?=C:\\Program Files\\SideShift".into()]));
        assert!(driven_by_installer(&["/UPDATE".into(), "/P".into()]));
        // 子进程标记必须落在 ① 而不是 ②：它一旦被判成"安装程序调起"，界面就永远出不来
        assert!(!driven_by_installer(&[CHILD.into(), "E:\\SideShift".into()]));
        assert!(!driven_by_installer(&[]));
        // 路径里凑巧含 P/S 的裸参数不算（② 只在整 token 相等时命中）
        assert!(!driven_by_installer(&["E:\\SP\\SideShift".into()]));
    }

    #[test]
    fn update_flag_added_once_and_only_for_installer_driven() {
        assert!(needs_update_flag(&["_?=E:\\SideShift".into()]));
        assert!(!needs_update_flag(&["_?=E:\\SideShift".into(), "/UPDATE".into()]));
        assert!(!needs_update_flag(&["/S".into()]), "人自己跑的静默卸载不能被判成升级");
    }

    #[test]
    fn first_token_stripped_without_touching_the_rest() {
        assert_eq!(
            strip_first_token("\"C:\\Program Files\\SideShift\\SideShift-Uninstall.exe\" _?=C:\\Program Files\\SideShift"),
            "_?=C:\\Program Files\\SideShift"
        );
        assert_eq!(strip_first_token("uninstall.exe /S"), "/S");
        assert_eq!(strip_first_token("uninstall.exe"), "");
        // 引号没闭合时宁可丢掉参数，也不能把整条命令行（含我们自己的路径）转出去
        assert_eq!(strip_first_token("\"abc"), "");
    }

    /// 老布局那两个目录由 identifier 拼出来：钩子删的是同一个名字，两边不一致就是"只清一半"
    #[test]
    fn legacy_dirs_use_the_identifier() {
        for d in legacy_dirs("com.poso.sideshift") {
            assert_eq!(d.file_name().and_then(|s| s.to_str()), Some("com.poso.sideshift"));
        }
        // 本机必然有 APPDATA/LOCALAPPDATA 两个变量。空 = 变量名写错，而不是"这台机器特殊"
        assert_eq!(legacy_dirs("x").len(), 2);
    }

    /// 原生入口被安装壳收进配置目录了，安装目录里那份老位置也要继续认：只认新位置等于
    /// 把"收失败过一次"（目标被占用）或"这版之前装的"那两套安装报成「找不到安装位置」，
    /// 而那一页是没有出口的
    #[test]
    fn nsis_entry_accepts_both_locations_new_first() {
        let dir = scratch("nsis-entry");
        assert_eq!(nsis_entry(&dir), None, "两处都没有就没有入口");

        let legacy = dir.join(data_root::NSIS_UNINSTALLER_NAME);
        std::fs::write(&legacy, b"x").unwrap();
        assert_eq!(nsis_entry(&dir), Some(legacy.clone()), "老位置那份得认");

        let stashed = dir
            .join(data_root::APP_DATA_DIR_NAME)
            .join(data_root::NSIS_UNINSTALLER_STASHED);
        std::fs::create_dir_all(stashed.parent().unwrap()).unwrap();
        std::fs::write(&stashed, b"x").unwrap();
        assert_eq!(nsis_entry(&dir), Some(stashed), "两份都在时认收起来那份");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 失败文案必须点名还剩什么：只有一句"被占用"的话，人不知道去哪找那个占用者
    #[test]
    fn residue_names_what_is_left() {
        let dir = std::env::temp_dir().join(format!("sideshift-residue-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), b"x").unwrap();
        let msg = residue(&[dir.clone(), dir.join("没有这个目录")]);
        assert!(msg.contains("还开着"), "少了原因那一句：{msg}");
        assert!(msg.contains(&display(&dir)), "没点名还剩哪个目录：{msg}");
        std::fs::remove_dir_all(&dir).ok();
    }

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sideshift-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 删缓存是整个改动里唯一不可逆的一刀，四条判据逐条测：`remove_dir_all` 没有第二次机会
    #[test]
    fn cache_deletion_never_trusts_a_loose_path() {
        let home = PathBuf::from(if cfg!(windows) { "C:\\Users\\ban" } else { "/home/ban" });
        let root = home.join("SideShift");
        let out = root.join("output");
        assert!(
            deletable(&root.join("cache"), &out, &home),
            "正常那一对（缓存是产物的兄弟目录）必须删得掉，否则这次改动等于没做"
        );
        assert!(!deletable(Path::new("cache"), &out, &home), "相对路径不知道相对于谁");
        assert!(
            !deletable(Path::new(if cfg!(windows) { "E:\\" } else { "/" }), &out, &home),
            "盘根不是缓存目录"
        );
        assert!(!deletable(&home, &out, &home), "用户目录更不是");
        assert!(!deletable(&out, &out, &home), "两个目录指向同一处：产物优先");
        assert!(
            !deletable(&root, &out, &home),
            "产物建在缓存底下：删缓存会把产物一起端走"
        );
    }

    /// 人在设置里改过缓存目录 ⇒ 默认布局算出来的那个 `cache` 就不是它，照着删是删错目录
    #[test]
    fn scene_takes_the_paths_from_settings_over_the_rule() {
        let install = scratch("scene-settings");
        let appdata = install.join(data_root::APP_DATA_DIR_NAME);
        std::fs::create_dir_all(&appdata).unwrap();
        let chosen = install.join("shared").join("mc-cache");
        std::fs::write(
            appdata.join(data_root::SETTINGS_FILE),
            format!(
                r#"{{"outputDir":"","cacheDir":{}}}"#,
                serde_json::to_string(&chosen.display().to_string()).unwrap()
            ),
        )
        .unwrap();

        let sc = scene(&install, "com.poso.sideshift").expect("临时目录里读得出现场");
        assert_eq!(sc.cache, chosen, "设置里那个路径才是这台机器的缓存");
        assert!(sc.deletable, "自定义缓存离产物足够远，该删");

        std::fs::remove_dir_all(&install).ok();
    }

    /// 配置目录那两档：应用写过设置的地方才算数，安装目录那份优先（它才是安装版的主档）
    #[test]
    fn config_dir_prefers_the_one_holding_settings() {
        let install = scratch("config-dir");
        let appdata = install.join(data_root::APP_DATA_DIR_NAME);
        assert_eq!(
            config_dir_of(&install, "com.poso.sideshift"),
            appdata,
            "哪份设置都没有时按安装版那一档走（installer.json 在那儿）"
        );
        std::fs::create_dir_all(&appdata).unwrap();
        std::fs::write(appdata.join(data_root::SETTINGS_FILE), "{}").unwrap();
        assert_eq!(config_dir_of(&install, "com.poso.sideshift"), appdata);
        std::fs::remove_dir_all(&install).ok();
    }

    #[test]
    fn only_an_absolute_saved_path_overrides_the_rule() {
        let fb = PathBuf::from(if cfg!(windows) { "F:\\SideShift\\cache" } else { "/mc/cache" });
        assert_eq!(absolute_or("   ".into(), fb.clone()), fb, "空串是没改过，不是改成了空目录");
        assert_eq!(absolute_or("relative/cache".into(), fb.clone()), fb, "相对路径不可信");
        // 带首尾空格的绝对路径：trim 之后仍然要采信（settings.json 里存过什么格式不由壳决定）
        let padded = format!("  {}  ", fb.display());
        assert_eq!(absolute_or(padded, fb.clone()), fb);
    }

    /// 进度是卸载中唯一的反馈：分子不能大于分母，也不能倒挂
    #[test]
    fn pct_reads_bytes_left_as_share_of_total() {
        let t = Instant::now();
        assert_eq!(pct_of(1000, 1000, t), 0.0);
        assert_eq!(pct_of(1000, 250, t), 75.0);
        assert_eq!(pct_of(1000, 0, t), 100.0);
        // 量不到字节（目录本来就空）时不许报满：宁停在 95% 让人等那一下
        assert!(pct_of(0, 0, t) < 95.0);
    }
}
