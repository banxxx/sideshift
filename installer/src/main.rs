//! SideShift 安装壳：界面与偏好归这里，**真正的落盘交给官方 NSIS**（静默跑内嵌的 `Setup.exe /S /D=...`）。
//! 壳只做两件事：① 问清楚数据要放哪；② 装完后把答案写进应用配置目录的 `installer.json`。
//! 数据根的最终裁决权在主应用（`core::data_root::suggested_root`）：这里写的只是**偏好**，路径失效时应用自行回落预选。

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

// 卸载壳的投递与注册表入口维护：与 data_root 同一条 #[path] 复用路线。
// 主应用（更新后自愈）与本壳（首装）是它的两个消费方，改行为两处同时生效
#[allow(dead_code)]
#[path = "../../src-tauri/src/core/uninstall_shell.rs"]
mod uninstall_shell;

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, State};

/// 内嵌的官方安装包（build.rs 从主应用的 nsis 产物里读进来）
static SETUP: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/setup-payload.bin"));

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
    // 点出来的那个窗口画成刚才用户看到的这套。落不进文件就 entirely 不碰注册表——原生入口至少能用。
    // 配置目录在这里先拼出来：deliver 收存原生卸载器时要把文件挪进去（目录由收存步骤自己建）
    let config = install_dir.join(data_root::APP_DATA_DIR_NAME);
    let note = match uninstall_shell::deliver(&uninstall_shell::NAMES, &install_dir, &config) {
        uninstall_shell::Deliver::Already | uninstall_shell::Deliver::Retargeted => None,
        uninstall_shell::Deliver::Note(m) => Some(m),
    };

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
    for dir in [&root, &output_dir] {
        std::fs::create_dir_all(dir).map_err(|e| {
            format!("建目录 {} 失败：{e}（程序已装好，装完后在设置里另选目录即可）", dir.display())
        })?;
    }
    // 缓存目录走 claim 而不是 create_dir_all：这一趟是它第一次出现在这台机器上，
    // 顺手盖上归属标记，卸载壳才认它是我们名下的（判据见 data_root::claim_cache_root）
    data_root::claim_cache_root(&cache_dir).map_err(|e| {
        format!("建目录 {} 失败：{e}（程序已装好，装完后在设置里另选目录即可）", cache_dir.display())
    })?;
    pct(app, Stage::Data, 94.0);

    // 写进**应用将来会用的那个配置目录**（上面那行已拼出路径）：安装版把配置放在 exe 同级的
    // `appdata`（判定在 `task_engine::persist::config_dir`，目录名单源是 `data_root::APP_DATA_DIR_NAME`）。
    // 不能再问 Tauri 要 app_config_dir()——那是老布局的 %APPDATA%，应用升级后已经不读它了。
    // 建不起来就吵一句：静默写不进去等于「用户在安装界面挑的数据目录白挑」，那笔账没法查
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
