//! SideShift 安装壳：界面与偏好归这里，**真正的落盘交给官方 NSIS**。
//!
//! 为什么不是壳自己去复制文件、建快捷方式、写卸载注册表：那三件事都得重做一遍才谈得上
//! "看起来像 SideShift"，而做坏了就是用户机器上删不掉的残留。所以壳只做两件事——
//! ① 问清楚数据要放哪；② 装完后把答案写进应用的配置目录（`installer.json`）。
//! 安装本身 = 静默跑内嵌的 `Setup.exe /S /D=...`，快捷方式/卸载登记/升级路径仍是 NSIS 那一套。
//!
//! 数据根的最终裁决权在主应用（`core::data_root::suggested_root`）：这里写的只是**偏好**，
//! 盘被拔掉或路径写坏了，应用会回落预选，不会把人锁在门外。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// 发布产物必须走 Tauri CLI（`pnpm installer`），不能裸 `cargo build --release`。
// 后者会让 tauri-build 给本 crate 打上 `cfg(dev)`，`generate_context!` 就不内嵌前端，
// 运行期改连 devUrl ⇒ 窗口开起来显示「无法访问页面」，而编译一路绿灯、字节数只差一点，
// 光看构建输出根本发现不了。dev 模式（debug + cfg(dev)）是合法的，所以再加一层 debug 判定
#[cfg(all(dev, not(debug_assertions)))]
compile_error!("安装壳请用 `pnpm installer` 出包（裸 cargo build --release 会带上 cfg(dev)，产物打不开页面）");

// 候选盘探测与数据根布局跟主应用共用同一份源码（同一个文件的字节，不是抄一份）：
// 「安装时显示的目录」和「应用实际用的目录」必须来自同一个规则
//
// 壳只走"安装器指定"这一条链：文件里那半套（便携判定、suggested_root 的回落顺序）在这里没人调。
// 但不为壳给共享源码加 cfg 去切它——整模块 allow(dead_code) 是这条源码级复用路线的固定代价
#[allow(dead_code)]
#[path = "../../src-tauri/src/core/data_root.rs"]
mod data_root;

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, State};

/// 内嵌的官方安装包（build.rs 从主应用的 nsis 产物里读进来）
static SETUP: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/setup-payload.bin"));

/// 内嵌的卸载壳（build.rs 从 `pnpm uninstaller` 的产物里读进来）。
/// NSIS 只登记它自己的 `uninstall.exe`，所以这份字节由壳落到安装目录、再把卸载入口改指过来
static UNINSTALL_SHELL: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/uninstall-shell.bin"));

/// 装完后主程序的期望字节数（进度条分母）。build.rs 拿不到时为 0 → 进度退化成时间爬升
const INSTALLED_EXE_BYTES: u64 = match option_env!("INSTALLED_EXE_BYTES") {
    Some(v) => parse_u64(v),
    None => 0,
};

const fn parse_u64(s: &str) -> u64 {
    let b = s.as_bytes();
    let mut n = 0u64;
    let mut i = 0;
    while i < b.len() {
        n = n * 10 + (b[i] - b'0') as u64;
        i += 1;
    }
    n
}

/// 三个可观测阶段。NSIS 静默模式不对外吐进度，所以**阶段边界只能划在壳自己看得见的地方**：
/// 解包、跑安装包、建数据目录。快捷方式与卸载登记发生在 NSIS 内部，壳看不见也管不着，
/// 因此不单独列一项——列了就是假进度。
#[derive(Serialize, Clone, Copy)]
#[serde(rename_all = "kebab-case")]
enum Stage {
    Prepare,
    Copy,
    Data,
}

impl Stage {
    fn as_str(self) -> &'static str {
        match self {
            Stage::Prepare => "prepare",
            Stage::Copy => "copy",
            Stage::Data => "data",
        }
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Progress {
    stage: &'static str,
    pct: f64,
    done: bool,
}

/// 候选盘档位：界面只用得上标签、剩余空间和数据根。产物/缓存目录由 `layout_in` 从数据根
/// 推出来，不在这份契约里复述。字节数交给前端格式化，避免两边四舍五入打架
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Offer {
    label: String,
    free_bytes: u64,
    data_root: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Plan {
    version: String,
    /// 架构标记：壳是单架构产物，摆在流程条上才知道手里这个 exe 对不对
    arch: String,
    /// 够用的非系统固定盘，按剩余空间降序；第一个就是预选档
    drives: Vec<Offer>,
    /// 没有候选盘时的回落数据根（用户目录下）
    fallback_data_root: String,
    /// 程序安装位置：与 NSIS 模板 currentUser 的默认值同源（`$LOCALAPPDATA\${PRODUCTNAME}`）
    install_dir: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Request {
    data_root: String,
    install_dir: String,
}

/// 装完之后界面要复述的两条路径：程序 exe 与实际生效的数据根。产物与缓存目录由同一个
/// `layout_in` 从数据根推出来，界面不逐条列——一屏里把"数据在哪"说三遍读起来像没结论
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Outcome {
    installed_exe: String,
    data_root: String,
    /// 卸载入口没能换成壳时的那句话（None = 已经指向壳）。装是装成了，所以它不能当失败处理，
    /// 但"控制面板里点出来的还是那个原生卸载对话框"这件事必须让人知道
    uninstall_note: Option<String>,
}

/// 取消位：UI 的「取消安装」写，安装线程每轮询一次读。
/// 包在 `Arc` 里才进得了 `spawn_blocking`——`State<'_, _>` 是借来的，闭包要的是 `'static`
#[derive(Clone, Default)]
struct Cancel(Arc<AtomicBool>);

#[tauri::command]
fn get_plan(app: AppHandle) -> Plan {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    let drives = data_root::offers()
        .into_iter()
        .map(|o| Offer {
            label: o.label,
            free_bytes: o.free_bytes,
            data_root: display(&o.data_root),
        })
        .collect();
    Plan {
        version: app.package_info().version.to_string(),
        arch: arch_label(),
        drives,
        fallback_data_root: display(&home.join(data_root::DIR_NAME)),
        install_dir: display(&default_install_dir()),
    }
}

/// 用户挑的目录 → 实际生效的数据根。用户自己挑时也走这里：
/// 「装完会不会变成另一套路径」这个风险只存在于两边各拼一次字符串的时候
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Layout {
    data_root: String,
}

#[tauri::command]
fn resolve_layout(root: String) -> Result<Layout, String> {
    let trimmed = trim_arg(&root);
    if trimmed.is_empty() {
        return Err("数据目录不能为空".into());
    }
    Ok(Layout {
        data_root: display(&PathBuf::from(trimmed)),
    })
}

/// 默认安装位置。刻意与 NSIS 模板 currentUser 分支同值：
/// 用户什么都不改时 `/D=` 传进去的和它自己的默认值一致，不会让下次升级换了目录
fn default_install_dir() -> PathBuf {
    std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir())
        .join(data_root::DIR_NAME)
}

/// Rust 的架构名 → 安装界面那三个字母（`x86_64` 摆在流程条里读起来像没翻译过）
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

#[tauri::command]
fn cancel_install(cancel: State<'_, Cancel>) {
    cancel.0.store(true, Ordering::SeqCst);
}

/// 安装是纯阻塞 IO（等子进程、遍历目录），丢到 blocking 线程池里跑：
/// 卡在 UI 线程上就是整个窗口不响应，这个项目为这件事翻过一次车
#[tauri::command]
async fn run_install(
    app: AppHandle,
    cancel: State<'_, Cancel>,
    req: Request,
) -> Result<Outcome, String> {
    cancel.0.store(false, Ordering::SeqCst);
    let sink = app.clone();
    let flag = cancel.0.clone();
    tauri::async_runtime::spawn_blocking(move || install(&sink, &flag, req))
        .await
        .map_err(|e| format!("安装线程异常退出：{e}"))?
}

/// 装完顺手把应用拉起来（「立即打开 SideShift」）
#[tauri::command]
fn launch_app(path: String) -> Result<(), String> {
    std::process::Command::new(&path)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("启动 {path} 失败：{e}"))
}

fn pct(app: &AppHandle, stage: Stage, value: f64) {
    let _ = app.emit(
        "installer://progress",
        Progress { stage: stage.as_str(), pct: value.clamp(0.0, 100.0), done: false },
    );
}

fn install(app: &AppHandle, cancel: &AtomicBool, req: Request) -> Result<Outcome, String> {
    let started = Instant::now();
    let install_dir = PathBuf::from(trim_arg(&req.install_dir));
    let root = PathBuf::from(trim_arg(&req.data_root));

    // ---- 阶段 1：内嵌的安装包落到临时目录 ----
    pct(app, Stage::Prepare, 2.0);
    let tmp = std::env::temp_dir().join(format!("sideshift-setup-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).map_err(|e| format!("建临时目录 {} 失败：{e}", tmp.display()))?;
    let setup = tmp.join("Setup.exe");
    std::fs::write(&setup, SETUP).map_err(|e| format!("写入安装包失败：{e}"))?;
    pct(app, Stage::Prepare, 8.0);
    if cancel.load(Ordering::SeqCst) {
        cleanup(&tmp);
        return Err("已取消安装".into());
    }

    // ---- 阶段 2：静默跑官方 NSIS ----
    let spawned = std::process::Command::new(&setup)
        .args(["/S", &format!("/D={}", install_dir.display())])
        .spawn();
    let mut child = match spawned {
        Ok(c) => c,
        Err(e) => {
            cleanup(&tmp);
            return Err(format!("启动安装程序失败：{e}"));
        }
    };
    let status = loop {
        if cancel.load(Ordering::SeqCst) {
            let _ = child.kill();
            let _ = child.wait();
            cleanup(&tmp);
            return Err("已取消安装".into());
        }
        match child.try_wait().map_err(|e| format!("等待安装程序失败：{e}"))? {
            Some(s) => break s,
            None => {
                pct(app, Stage::Copy, copy_pct(&install_dir, started));
                std::thread::sleep(Duration::from_millis(150));
            }
        }
    };
    if !status.success() {
        cleanup(&tmp);
        return Err(format!(
            "安装程序未成功结束（退出码 {}）",
            status.code().unwrap_or(-1)
        ));
    }
    pct(app, Stage::Copy, 88.0);

    // 卸载入口换成壳自己那一份：NSIS 登记的永远是它自己的 `uninstall.exe`，而壳要把控制面板里
    // 点出来的那个窗口画成刚才用户看到的这套。落不进文件就 entirely 不碰注册表——原生入口至少能用
    let note = deliver_uninstall_shell(&install_dir);

    // ---- 阶段 3：数据目录 + 把偏好交给应用 ----
    let exe = install_dir.join(format!("SideShift{}", std::env::consts::EXE_SUFFIX));
    if !exe.is_file() {
        cleanup(&tmp);
        return Err(format!(
            "安装程序报告成功，但 {} 不在——安装目录可能被杀毒软件清掉了",
            exe.display()
        ));
    }
    let (output_dir, cache_dir) = data_root::layout_in(&root);
    for dir in [&root, &output_dir, &cache_dir] {
        std::fs::create_dir_all(dir).map_err(|e| {
            format!("建目录 {} 失败：{e}（程序已装好，装完后在设置里另选目录即可）", dir.display())
        })?;
    }
    pct(app, Stage::Data, 94.0);

    // 写进**应用将来会用的那个配置目录**：安装版把配置放在 exe 同级的 `appdata`
    // （判定在 `task_engine::persist::config_dir`，目录名单源是 `data_root::APP_DATA_DIR_NAME`）。
    // 不能再问 Tauri 要 app_config_dir()——那是老布局的 %APPDATA%，应用升级后已经不读它了。
    // 建不起来就吵一句：静默写不进去等于「用户在安装界面挑的数据目录白挑」，那笔账没法查
    let config = install_dir.join(data_root::APP_DATA_DIR_NAME);
    std::fs::create_dir_all(&config)
        .map_err(|e| format!("建配置目录 {} 失败：{e}（程序已装好，但数据目录的选择没落下来）", config.display()))?;
    let target = config.join(data_root::INSTALLER_FILE);
    let body = serde_json::json!({ "dataRoot": display(&root) }).to_string();
    std::fs::write(&target, &body).map_err(|e| format!("写 {} 失败：{e}", target.display()))?;

    cleanup(&tmp);
    let _ = app.emit(
        "installer://progress",
        Progress { stage: Stage::Data.as_str(), pct: 100.0, done: true },
    );

    Ok(Outcome {
        installed_exe: display(&exe),
        data_root: display(&root),
        uninstall_note: note,
    })
}

/// 卸载入口现在指向谁。判定只看**文件名**，不比整条路径：`$INSTDIR` 由 NSIS 自己拼，
/// 尾随分隔符和大小写都不由我们做主，比整串等于 `"$INSTDIR\uninstall.exe"` 会误判成"不是我们的键"
#[cfg(windows)]
enum Entry {
    /// 原生入口 → 改成壳
    Rewrite(String),
    /// 已经是壳（升级走这条）→ 什么都不动
    Already,
    /// 空 / 读不到 / 指向别处 → 不碰，让原生那条路留着
    Foreign,
}

/// 纯判定部分，单独抽出来才能测（注册表那两个调用没有可测的余地）
#[cfg(windows)]
fn classify_entry(existing: Option<&str>, shell_path: &Path) -> Entry {
    let Some(raw) = existing.map(str::trim).filter(|s| !s.is_empty()) else {
        return Entry::Foreign;
    };
    let name_of = |p: &str| {
        p.rsplit(['\\', '/'])
            .next()
            .unwrap_or("")
            .to_ascii_lowercase()
    };
    // 模板写进去的值是带引号的 `"$INSTDIR\uninstall.exe"`，读回来原样带着那对引号
    let stem = name_of(raw.trim_matches('"'));
    if stem == name_of(&display(shell_path)) {
        Entry::Already
    } else if stem == "uninstall.exe" {
        Entry::Rewrite(format!("\"{}\"", display(shell_path)))
    } else {
        Entry::Foreign
    }
}

/// 把卸载壳落到安装目录，并把注册表里的卸载入口改指过去。
///
/// 返回 `Some(一句话)` 表示"没换成"，但**安装本身照旧成功**：原生 `uninstall.exe` 还在原位，
/// 控制面板里仍然卸得掉，只是那套界面是 NSIS 的。所以这是一条 note，不是一次失败——
/// 把装好了的东西报成失败，比让人多看到一个原生对话框严重的多
#[cfg(windows)]
fn deliver_uninstall_shell(install_dir: &Path) -> Option<String> {
    let target = install_dir.join(data_root::UNINSTALL_SHELL_NAME);
    if let Err(e) = std::fs::write(&target, UNINSTALL_SHELL) {
        return Some(format!(
            "卸载界面没换成自带的那套：写 {} 失败（{e}）。控制面板里的卸载仍然可用",
            target.display()
        ));
    }
    match retarget_entry(&target) {
        // Already = 升级覆盖，Rewrite = 刚改指过来。两种都是"入口就是它"
        Ok(Entry::Already) | Ok(Entry::Rewrite(_)) => None,
        Ok(Entry::Foreign) => Some(
            "卸载界面没换成自带的那套：注册表里的卸载入口不是这个安装目录写的，没有去改它。控制面板里的卸载仍然可用"
                .into(),
        ),
        Err(e) => Some(format!("卸载界面没换成自带的那套：{e}。控制面板里的卸载仍然可用")),
    }
}

#[cfg(not(windows))]
fn deliver_uninstall_shell(_install_dir: &Path) -> Option<String> {
    // 非 Windows 不产出安装包，这条链不走；留个桩是为了 data_root 那份共享源码能在本机编过
    None
}

/// `UNINSTKEY` = `Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCTNAME}`
/// （生成脚本 :61）。产品名取自 src-tauri/tauri.conf.json，改那一边要同步这一边——
/// 不同步的后果是"谁都改不到"，卸载入口保持原生，属于安全的那一侧
#[cfg(windows)]
const UNINST_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\SideShift";

/// 一次开键、读现值、判定、必要时写。读之前不写：现值就是"这个键是不是我们装的"的唯一凭据
#[cfg(windows)]
fn retarget_entry(shell: &Path) -> Result<Entry, String> {
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
        KEY_READ, KEY_SET_VALUE, REG_SZ,
    };

    let key = wide(UNINST_KEY);
    let name = wide("UninstallString");
    let mut h: HKEY = std::ptr::null_mut();
    unsafe {
        let opened = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            0,
            KEY_READ | KEY_SET_VALUE,
            &mut h,
        );
        if opened == ERROR_FILE_NOT_FOUND {
            return Ok(Entry::Foreign);
        }
        if opened != ERROR_SUCCESS {
            return Err(format!("打开卸载注册表键失败（{opened}）"));
        }

        // 先量长度再读：RegQueryValueExW 的两段式调用没有别的写法
        let mut len = 0u32;
        let mut kind = 0u32;
        let probe = RegQueryValueExW(
            h,
            name.as_ptr(),
            std::ptr::null(),
            &mut kind,
            std::ptr::null_mut(),
            &mut len,
        );
        let mut existing = None;
        if probe == ERROR_SUCCESS && len > 0 && kind == REG_SZ {
            let mut buf = vec![0u16; len as usize / 2 + 1];
            let mut got = len;
            let read = RegQueryValueExW(
                h,
                name.as_ptr(),
                std::ptr::null(),
                &mut kind,
                buf.as_mut_ptr() as *mut u8,
                &mut got,
            );
            if read == ERROR_SUCCESS {
                let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
                existing = Some(String::from_utf16_lossy(&buf[..end]));
            }
        }

        let verdict = classify_entry(existing.as_deref(), shell);
        let mut failure = None;
        if let Entry::Rewrite(value) = &verdict {
            let w = wide(value);
            // cbData 含结尾那个 NUL，和 NSIS 自己写这条值时的算法一致
            let set = RegSetValueExW(
                h,
                name.as_ptr(),
                0,
                REG_SZ,
                w.as_ptr() as *const u8,
                (w.len() * 2) as u32,
            );
            if set != ERROR_SUCCESS {
                failure = Some(format!("写 UninstallString 失败（{set}）"));
            }
        }
        RegCloseKey(h);
        match failure {
            Some(e) => Err(e),
            None => Ok(verdict),
        }
    }
}

#[cfg(windows)]
fn wide(s: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// 复制阶段的百分比：以安装目录实际字节数为准（真在涨才算进度）。
/// 没有期望字节数时退化成"越等越慢的爬升"，同样封顶在 80% ⇒ 宁可不满也不报假完成
fn copy_pct(install_dir: &Path, started: Instant) -> f64 {
    const BASE: f64 = 8.0;
    const CEIL: f64 = 80.0;
    let expected = INSTALLED_EXE_BYTES + 1_500_000;
    let ratio = if expected > 0 {
        (dir_bytes(install_dir) as f64 / expected as f64).min(1.0)
    } else {
        1.0 - (-(started.elapsed().as_secs_f64()) / 12.0).exp()
    };
    BASE + (CEIL - BASE) * ratio
}

/// 目录内文件字节总和：安装目录里只有主程序与卸载器，遍历成本可忽略
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

fn cleanup(tmp: &Path) {
    let _ = std::fs::remove_dir_all(tmp);
}

/// 用户手打的目录可能带引号或首尾空格，NSIS 的 `/D=` 对此很敏感。
/// 只剥引号与首尾空白，**不动尾部分隔符**：盘根 `D:\` 被削成 `D:` 就装错地方了
fn trim_arg(s: &str) -> String {
    s.trim().trim_matches('"').trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn entry_is_rewritten_only_from_the_native_one() {
        let shell = Path::new("E:\\SideShift\\SideShift-Uninstall.exe");
        // 模板写的值带引号，且 $INSTDIR 的分隔符尾注不确定 → 只认文件名
        let native = "\"E:\\SideShift\\uninstall.exe\"".to_string();
        let Entry::Rewrite(v) = classify_entry(Some(&native), shell) else {
            panic!("原生入口必须改成壳");
        };
        assert_eq!(v, "\"E:\\SideShift\\SideShift-Uninstall.exe\"");
        // 升级覆盖时读回来的已经是壳：不能再改一次，也不该报任何话
        assert!(matches!(
            classify_entry(Some("\"E:\\SideShift\\SideShift-Uninstall.exe\""), shell),
            Entry::Already
        ));
        // 大小写与正斜杠都不是判据（同一台机器上两种写法都合法）
        assert!(matches!(
            classify_entry(Some("e:/side shift/UNINSTALL.EXE"), shell),
            Entry::Rewrite(_)
        ));
        // 别的程序占着同一个键名 ⇒ 一个字都不改。注意判据是**文件名**而不是整条路径：
        // 这条 UninstallString 是刚才 NSIS 自己写的（模板 :677），目录必然就是这次装的那个，
        // 而路径字符串的写法（尾分隔符、大小写、正反斜杠）我们做不了主
        assert!(matches!(
            classify_entry(Some("\"C:\\Other\\uninstall.exe\""), shell),
            Entry::Rewrite(_)
        ));
        assert!(matches!(classify_entry(Some("\"C:\\Other\\setup.exe\""), shell), Entry::Foreign));
        assert!(matches!(classify_entry(Some("\"C:\\Other\\setup.exe\""), shell), Entry::Foreign));
        assert!(matches!(classify_entry(None, shell), Entry::Foreign));
        assert!(matches!(classify_entry(Some("   "), shell), Entry::Foreign));
    }

    /// 静默安装的目录归一：尾部分隔符要留着（盘根 `D:\` 削成 `D:` 会装错地方），引号要剥掉
    #[test]
    fn trim_arg_keeps_the_trailing_separator() {
        assert_eq!(trim_arg(" \"D:\\SideShift\" "), "D:\\SideShift");
        assert_eq!(trim_arg("D:\\"), "D:\\");
    }
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Cancel::default())
        .invoke_handler(tauri::generate_handler![
            get_plan,
            resolve_layout,
            run_install,
            cancel_install,
            launch_app
        ])
        .run(tauri::generate_context!())
        .expect("SideShift 安装壳启动失败");
}
