//! 本机开服验证的会话层：单槽位调度 + 增量事件 + 会话落盘。
//! 执行体在 `core::verify_run`（纯回调、不认 tauri），这一层只管状态表、事件与存档。
//!
//! 落盘节奏：结构化快照（`.verify-run.json`）与控制台转写（`server.log`）都写在**副本根**，
//! 每换一步写一次、日志每积 50 行再写一次。所以进程被强杀时最多丢最后几十行原文，
//! 页面重开仍能认回上次跑到哪一步；而没解出副本的那两档（不可判定 / 交付包不在）根本没有
//! 目录可写 ⇒ 不留档，重启后回到「还没跑过」。
//!
//! 副本的寿命与结论绑在一起：跑完（含通过）不删，页面下次进来直接演最近这一轮的结果；
//! 重新验证是**覆盖**同一份快照，不留历史。只有「清理验证目录」与彻底丢弃任务（清空回收站）
//! 两处真删目录——快照就在目录里，删了它这一轮结果也就没了，回到「还没跑过」。

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::core::verify_copy;
use crate::core::verify_run::{self, RunInput, VerifyEvent, VERIFY_TIMEOUT};
use crate::l10n::Msg;
use crate::msg;
use crate::models::*;
use super::events::{truncation_msg, MAX_LOG_LINES};
use super::state::AppState;
use super::util::{now_hms, now_ms};

/// 进度事件：只带增量（一行日志或一步换态），终态由页面回拉整份快照——与转换那条链同口径
pub const EVENT_PROGRESS: &str = "verify://progress";
/// 一轮验证结束（通过/失败/超时/取消都发）
pub const EVENT_DONE: &str = "verify://done";
/// 会话快照落盘名。以 `.` 开头 ⇒ 服务端与加载器都不会读它（与副本哨兵同一条规矩）
const SESSION_FILE: &str = ".verify-run.json";
/// 控制台转写：给「打开日志文件」用，内容与页面日志环逐行一致（同样只留最近 600 行）。
/// 文件名照稿（页面注记「完整输出写入 server.log」）
const LOG_FILE: &str = "server.log";
/// 落盘攒批的行数门槛：只在步变时写的话，等待就绪那一步能跑几分钟、几千行原文全在内存里
const FLUSH_EVERY_LINES: usize = 50;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct VerifyProgress {
    pub task_id: String,
    pub status: VerifyStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<VerifyStep>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log: Option<VerifyLogLine>,
}

/// 起一轮验证要的全部输入（锁内取好、锁外用）：执行体拿的都是快照副本，
/// 跑的这一会儿任务本身可能被人改名、删掉甚至重跑
struct Launch {
    cache_dir: PathBuf,
    zip_path: Option<PathBuf>,
    options: ConversionOptions,
    loader: LoaderKind,
    checks: Vec<CheckResult>,
    installed: bool,
    cancel: Arc<AtomicBool>,
}

/// 页面快照：本次会话跑过就回内存那份，否则回读副本根的存档，两头都没有才是 ready。
/// 回读的那份会补进内存表——模组启停要改的就是表里那几个计数，不能只改盘
pub fn session(state: &AppState, id: &str) -> VerifySession {
    // 锁内一次拿齐三份读参数：缓存目录、交付包（Ready 那一档的参数线索得从包里读）、构建报告的安装档
    let (cache_dir, zip_path, loader_installed) = {
        let inner = state.inner.lock().unwrap();
        (
            PathBuf::from(inner.settings.cache_dir.clone()),
            inner.tasks.get(id).and_then(|t| t.output_path.clone()),
            inner.reports.get(id).map(|r| r.installed),
        )
    };
    let found = state.inner.lock().unwrap().verifies.get(id).cloned();
    let mut s = match found {
        Some(s) => s,
        None => match load(&cache_dir, id) {
            Some(loaded) => {
                let out = loaded.clone();
                state.inner.lock().unwrap().verifies.insert(id.to_string(), loaded);
                out
            }
            None => VerifySession::ready(id, VERIFY_TIMEOUT.as_secs()),
        },
    };
    fill_params(&mut s, zip_path.as_deref(), loader_installed);
    s
}

/// 「验证参数」卡的三项，每次读快照现算：副本在就读副本那份（跑过一轮之后生效的是它），
/// 没有副本就读交付包原文，所以 Ready 那一档也能把这张卡列全。读不到留 None，界面写「未取到」
fn fill_params(
    s: &mut VerifySession,
    zip_path: Option<&Path>,
    loader_installed: Option<bool>,
) {
    s.timeout_sec = VERIFY_TIMEOUT.as_secs();
    s.loader_installed = loader_installed;
    let dir = Path::new(&s.copy_dir);
    let hints = if !s.copy_dir.is_empty() && dir.is_dir() {
        verify_copy::copy_hints(dir)
    } else {
        zip_path.map(verify_copy::pack_hints).unwrap_or_default()
    };
    s.jvm_xmx = hints.xmx;
    s.online_mode = hints.online_mode;
}

/// 存档回读。上一轮被关应用打断的那份还写着 `running`：进程早没了，当场改判并落回去，
/// 这条降级因此只做一次（否则每次进页面都重新判一遍、日志环里堆满同一句）
fn load(cache_dir: &Path, task_id: &str) -> Option<VerifySession> {
    let dir = verify_copy::copy_dir(cache_dir, task_id).ok()?;
    let text = fs::read_to_string(dir.join(SESSION_FILE)).ok()?;
    let mut s: VerifySession = serde_json::from_str(&text).ok()?;
    if s.status == VerifyStatus::Running {
        s.status = VerifyStatus::Cancelled;
        s.finished_at = Some(now_ms());
        push(
            &mut s,
            VerifySource::Env,
            LogLevel::Warn,
            msg!("应用已关闭，上一轮验证中断，本次不作判定"),
        );
        save(cache_dir, &s);
    }
    Some(s)
}

/// 快照与转写各写一份，都在副本根；目录还不存在（没解出副本）就一个字都不写
fn save(cache_dir: &Path, s: &VerifySession) {
    let Ok(dir) = verify_copy::copy_dir(cache_dir, &s.task_id) else {
        return;
    };
    if !dir.is_dir() {
        return;
    }
    if let Ok(text) = serde_json::to_string_pretty(s) {
        let _ = fs::write(dir.join(SESSION_FILE), text);
    }
    let _ = write_log_file(&dir, s);
}

/// 整份重写而不是追加：文件必须与页面日志环逐行一致，否则「打开日志文件」看到的是另一套账
fn write_log_file(dir: &Path, s: &VerifySession) -> std::io::Result<()> {
    let mut f = fs::File::create(dir.join(LOG_FILE))?;
    if (s.log_lines as usize) > s.logs.len() {
        writeln!(f, "{}", truncation_msg().zh)?;
    }
    for l in &s.logs {
        writeln!(f, "[{}] [{}] {}", l.time, source_tag(l.source), l.message)?;
    }
    f.flush()
}

fn source_tag(source: VerifySource) -> &'static str {
    match source {
        VerifySource::Env => "env",
        VerifySource::Unpack => "unpack",
        VerifySource::Verify => "verify",
        VerifySource::Loader => "loader",
        VerifySource::Java => "java",
        VerifySource::Server => "server",
        VerifySource::Mixin => "mixin",
    }
}

fn log_line(source: VerifySource, level: LogLevel, msg: Msg) -> VerifyLogLine {
    VerifyLogLine {
        time: now_hms(),
        source,
        message: msg.zh.clone(),
        message_msg: Some(msg),
        level,
    }
}

/// 日志环：与转换日志同一条预算（`events::MAX_LOG_LINES`）。这里只裁行数、不插截断标记——
/// 验证日志行带的是来源标签而不是流水线阶段，那句「仅保留最近 N 条」由 `log_lines` 与
/// `logs.len()` 的差在界面上算，写文件时补进开头
fn push(s: &mut VerifySession, source: VerifySource, level: LogLevel, msg: Msg) {
    s.logs.push(log_line(source, level, msg));
    if s.logs.len() > MAX_LOG_LINES {
        s.logs.remove(0);
    }
    s.log_lines += 1;
}

/// 开一轮验证。检查与登记在锁内一次做完（两处单槽位判据不能分两次拿锁，否则两条并发的
/// 开始请求会都看见「空槽」），解包与起进程一律在后台线程：那两段能抱着非重入锁自锁死
pub fn start(app: &AppHandle, state: &Arc<AppState>, id: &str) -> Result<(), String> {
    let launch = {
        let mut inner = state.inner.lock().unwrap();
        // 只读检查一律排在改表之前：这一整段都在同一把锁里，两处单槽位判据不能分两次拿
        let task = inner
            .tasks
            .get(id)
            .ok_or("这条任务已经不在了，没有可验证的交付包")?;
        if matches!(task.status, TaskStatus::Queued | TaskStatus::Running) {
            return Err("这条任务还在转换，等它出结果再验证".to_string());
        }
        let (zip_path, options, loader) = (
            task.output_path.clone(),
            task.options.clone(),
            task.pack.loader,
        );
        if inner.verify_current.is_some() {
            return Err("已经有一条验证在跑，先等它结束或把它停掉".to_string());
        }
        // 转换与验证共用同一份缓存目录，而且都吃满磁盘与网络：同一时间只走一条
        if inner.current.is_some() {
            return Err("有转换任务正在跑，先等它结束再验证".to_string());
        }
        let flag = inner
            .verify_cancel
            .entry(id.to_string())
            .or_insert_with(|| Arc::new(AtomicBool::new(false)))
            .clone();
        // 上一轮的取消标志必须清零，否则这一轮每个回调都被它按死
        flag.store(false, Ordering::Relaxed);
        let report = inner.reports.get(id);
        Launch {
            cache_dir: PathBuf::from(inner.settings.cache_dir.clone()),
            zip_path,
            options,
            loader,
            checks: report.map(|r| r.checks.clone()).unwrap_or_default(),
            installed: report.map(|r| r.installed).unwrap_or(false),
            cancel: flag,
        }
    };

    let fresh = VerifySession {
        // 六步一次摆齐：页面从第一帧起就有完整清单，走到的那几步再逐条改色
        steps: verify_run::new_steps(),
        status: VerifyStatus::Running,
        started_at: Some(now_ms()),
        ..VerifySession::ready(id, VERIFY_TIMEOUT.as_secs())
    };
    {
        let mut inner = state.inner.lock().unwrap();
        inner.verifies.insert(id.to_string(), fresh);
        inner.verify_current = Some(id.to_string());
    }
    save(&launch.cache_dir, &session(state, id));
    spawn_verify(app.clone(), state.clone(), id.to_string(), launch);
    Ok(())
}

fn spawn_verify(app: AppHandle, state: Arc<AppState>, id: String, launch: Launch) {
    tauri::async_runtime::spawn(async move {
        let mut runner = Runner {
            app: app.clone(),
            state: state.clone(),
            id: id.clone(),
            cache_dir: launch.cache_dir.clone(),
            since_flush: 0,
        };
        let input = RunInput {
            cache_dir: &launch.cache_dir,
            task_id: &id,
            zip_path: launch.zip_path.as_deref(),
            options: &launch.options,
            loader: launch.loader,
            checks: &launch.checks,
            installed: launch.installed,
            cancel: &launch.cancel,
            timeout: VERIFY_TIMEOUT,
        };
        let res = verify_run::run(&input, &mut |ev| runner.on_event(ev));
        runner.finish(&res);
    });
}

/// 事件回写员：执行体每报一步/一行，这里改快照、播事件、按节奏落盘
struct Runner {
    app: AppHandle,
    state: Arc<AppState>,
    id: String,
    cache_dir: PathBuf,
    since_flush: usize,
}

impl Runner {
    fn on_event(&mut self, ev: VerifyEvent) {
        let (progress, dump) = {
            let mut inner = self.state.inner.lock().unwrap();
            // 会话已经不在表里 = 任务被删掉了：不再播，执行体自己收尾
            let Some(s) = inner.verifies.get_mut(&self.id) else {
                return;
            };
            let (step, log, flush) = match ev {
                VerifyEvent::Step { id, status, note } => {
                    let step = s.steps.iter_mut().find(|x| x.id == id).map(|st| {
                        st.status = status;
                        st.note = note.as_ref().map(|m| m.zh.clone()).unwrap_or_default();
                        st.note_msg = note;
                        st.clone()
                    });
                    // 每步落定都写一次：六步最多 6 次，换来「中途被杀也有结论」
                    (step, None, status != StepStatus::Running)
                }
                VerifyEvent::Line { source, level, msg } => {
                    let line = log_line(source, level, msg);
                    s.logs.push(line.clone());
                    if s.logs.len() > MAX_LOG_LINES {
                        s.logs.remove(0);
                    }
                    s.log_lines += 1;
                    self.since_flush += 1;
                    let flush = self.since_flush >= FLUSH_EVERY_LINES;
                    if flush {
                        self.since_flush = 0;
                    }
                    (None, Some(line), flush)
                }
            };
            let dump = flush.then(|| s.clone());
            (
                VerifyProgress {
                    task_id: self.id.clone(),
                    status: s.status,
                    step,
                    log,
                },
                dump,
            )
        };
        let _ = self.app.emit(EVENT_PROGRESS, progress);
        if let Some(s) = dump {
            save(&self.cache_dir, &s);
        }
    }

    /// 收尾：结论、数字、单槽位交回，然后才发 done。落盘在发事件之前——
    /// 页面收到 done 就回拉快照，先落盘才拉得到这一轮的完整结论
    fn finish(&mut self, res: &verify_run::RunResult) {
        let counts = if res.copy_dir.as_os_str().is_empty() {
            None
        } else {
            Some(verify_copy::mod_counts(&res.copy_dir))
        };
        let snap = {
            let mut inner = self.state.inner.lock().unwrap();
            // 槽位先交回：会话随后可能改不到（任务已被删），那一档绝不能把槽位占死
            if inner.verify_current.as_deref() == Some(self.id.as_str()) {
                inner.verify_current = None;
            }
            let Some(s) = inner.verifies.get_mut(&self.id) else {
                return;
            };
            s.status = res.status;
            s.steps = res.steps.clone();
            s.elapsed_sec = res.elapsed.as_secs();
            s.finished_at = Some(now_ms());
            s.exit_code = res.exit_code;
            s.server_port = res.server_port;
            s.warnings = res.warnings.clone();
            s.failure = res.failure.clone();
            s.blockers = res.blockers.clone();
            s.hints = res.hints.clone();
            // 这一轮没解出副本（不可判定 / 交付包不在）时保留上一次的目录：它还在盘上
            if !res.copy_dir.as_os_str().is_empty() {
                s.copy_dir = res.copy_dir.to_string_lossy().to_string();
                s.copy_bytes = res.copy_bytes;
            }
            if let Some((total, disabled)) = counts {
                s.mod_total = total;
                s.mod_disabled = disabled;
            }
            s.clone()
        };
        save(&self.cache_dir, &snap);
        let _ = self.app.emit(
            EVENT_DONE,
            HashMap::from([("taskId".to_string(), self.id.clone())]),
        );
    }
}

/// 请求取消：只立旗并记一行，终态仍由执行体收（它要负责杀进程、写回退出码）。
/// 返回 false = 这一条现在根本没在跑
pub fn cancel(app: &AppHandle, state: &Arc<AppState>, id: &str) -> bool {
    let (emitted, cache_dir) = {
        let mut inner = state.inner.lock().unwrap();
        if inner.verify_current.as_deref() != Some(id) {
            return false;
        }
        if let Some(flag) = inner.verify_cancel.get(id) {
            flag.store(true, Ordering::Relaxed);
        }
        match inner.verifies.get_mut(id) {
            Some(s) => {
                push(
                    s,
                    VerifySource::Env,
                    LogLevel::Info,
                    msg!("已请求取消，服务端进程会被终止"),
                );
                (Some(s.clone()), inner.settings.cache_dir.clone())
            }
            None => (None, inner.settings.cache_dir.clone()),
        }
    };
    // 取消请求要立刻在日志里看得见，不等执行体那 150ms 一圈
    if let Some(s) = emitted {
        let _ = app.emit(
            EVENT_PROGRESS,
            VerifyProgress {
                task_id: id.to_string(),
                status: s.status,
                step: None,
                log: s.logs.last().cloned(),
            },
        );
        save(&PathBuf::from(cache_dir), &s);
    }
    true
}

/// 副本里的模组清单（启停弹窗的列表）。没有副本就是空表，不报错——页面按会话状态决定这颗按钮可不可点
pub fn mods(state: &AppState, id: &str) -> Result<Vec<VerifyModEntry>, String> {
    let cache_dir = PathBuf::from(state.inner.lock().unwrap().settings.cache_dir.clone());
    verify_copy::list_mods(&cache_dir, id)
}

/// 启停一枚模组：改名落在副本的 `mods/`，交付包与下载缓存一个字都不动。
/// 验证进行中禁改：服务端正读着这个目录，改完这一轮的结果归因不到模组上
pub fn toggle_mod(
    state: &Arc<AppState>,
    id: &str,
    file: &str,
    enabled: bool,
) -> Result<ModToggleResult, String> {
    let cache_dir = {
        let inner = state.inner.lock().unwrap();
        if inner.verify_current.as_deref() == Some(id) {
            return Err("这条任务正在验证，先停掉再改模组".to_string());
        }
        PathBuf::from(inner.settings.cache_dir.clone())
    };
    let entry = verify_copy::set_mod_enabled(&cache_dir, id, file, enabled)?;
    let mods = verify_copy::list_mods(&cache_dir, id)?;
    let total = mods.len() as u32;
    let disabled = mods.iter().filter(|m| !m.enabled).count() as u32;
    {
        let mut inner = state.inner.lock().unwrap();
        if let Some(s) = inner.verifies.get_mut(id) {
            s.mod_total = total;
            s.mod_disabled = disabled;
            // 「已启用/已停用」各占一条模板：把中文词当参数塞进同一句，英文档就会插出中文
            let note = if enabled {
                msg!("已启用 {{file}}，重新验证后生效", {"file": entry.file.clone()})
            } else {
                msg!("已停用 {{file}}，重新验证后生效", {"file": entry.file.clone()})
            };
            push(s, VerifySource::Unpack, LogLevel::Info, note);
            save(&cache_dir, s); // 这里在锁内写文件：一次改名 + 一份 600 行以内的快照，量级可忽略
        }
    }
    Ok(ModToggleResult {
        entry,
        mods,
        mod_total: total,
        mod_disabled: disabled,
    })
}

/// 「清理验证目录」：删掉副本整份，会话回到默认那一档。
/// 快照与日志就写在副本根里，目录没了它们跟着没——重新验证一次才有新结果，
/// 所以这里不做「保留结论、只清文件」那种半份状态。
/// 递归删必须在解锁之后做：这里自己锁一次取目录，与 `clear_trash` 同一规矩
pub fn clean_dir(app: &AppHandle, state: &Arc<AppState>, id: &str) -> Result<(), String> {
    let cache_dir = {
        let inner = state.inner.lock().unwrap();
        if inner.verify_current.as_deref() == Some(id) {
            return Err("这条任务正在验证，先停掉再清目录".to_string());
        }
        PathBuf::from(inner.settings.cache_dir.clone())
    };
    verify_copy::teardown(&cache_dir, id)?;
    state.inner.lock().unwrap().verifies.remove(id);
    // 与一轮跑完同一个事件：页面收到就回拉快照，拉回来的是默认那一档
    let _ = app.emit(
        EVENT_DONE,
        HashMap::from([("taskId".to_string(), id.to_string())]),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_file_matches_the_ring_when_trimmed() {
        let dir = std::env::temp_dir().join(format!("sideshift-verify-log-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let mut s = VerifySession::ready("task-a", VERIFY_TIMEOUT.as_secs());
        s.log_lines = MAX_LOG_LINES as u32 + 40;
        s.logs = (0..MAX_LOG_LINES)
            .map(|i| log_line(VerifySource::Server, LogLevel::Info, Msg::foreign(format!("line {i}"))))
            .collect();
        write_log_file(&dir, &s).unwrap();
        let text = fs::read_to_string(dir.join(LOG_FILE)).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), MAX_LOG_LINES + 1);
        assert!(lines[0].contains("仅保留最近"), "截断说明必须是第一行：{}", lines[0]);
        assert!(lines[1].starts_with("[") && lines[1].contains("[server]"), "{}", lines[1]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_is_a_noop_without_the_copy_dir() {
        let root = std::env::temp_dir().join(format!("sideshift-verify-save-{}", uuid::Uuid::new_v4()));
        let mut s = VerifySession::ready("task-b", VERIFY_TIMEOUT.as_secs());
        s.status = VerifyStatus::Blocked;
        save(&root, &s);
        assert!(!root.exists(), "没有副本就不该凭空造出目录");
    }

    #[test]
    fn ring_keeps_the_newest_lines_and_counts_everything() {
        let mut s = VerifySession::ready("task-c", VERIFY_TIMEOUT.as_secs());
        for i in 0..(MAX_LOG_LINES + 250) {
            push(&mut s, VerifySource::Server, LogLevel::Info, Msg::foreign(format!("line {i}")));
        }
        assert_eq!(s.logs.len(), MAX_LOG_LINES);
        assert_eq!(s.log_lines, (MAX_LOG_LINES + 250) as u32);
        assert_eq!(s.logs.last().unwrap().message, format!("line {}", MAX_LOG_LINES + 249));
        assert_eq!(s.logs.first().unwrap().message, "line 250");
    }

    #[test]
    fn source_tags_are_the_ones_the_mock_uses() {
        assert_eq!(source_tag(VerifySource::Env), "env");
        assert_eq!(source_tag(VerifySource::Mixin), "mixin");
    }

    /// 「验证参数」三项现算：副本在就读副本，没有副本也没有包就一律空，超时那一格恒与执行体同源
    #[test]
    fn params_read_from_the_copy_and_stay_empty_without_it() {
        let dir = std::env::temp_dir().join(format!("sideshift-verify-params-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("start.bat"),
            "@echo off\r\ncd /d \"%~dp0\"\r\njava -Xmx3G -jar server.jar nogui\r\n",
        )
        .unwrap();
        fs::write(dir.join("server.properties"), "online-mode=true\n").unwrap();

        let mut s = VerifySession::ready("task-d", VERIFY_TIMEOUT.as_secs());
        s.copy_dir = dir.to_string_lossy().to_string();
        fill_params(&mut s, None, Some(true));
        assert_eq!(s.jvm_xmx.as_deref(), Some("-Xmx3G"));
        assert_eq!(s.online_mode, Some(true));
        assert_eq!(s.loader_installed, Some(true));

        let mut none = VerifySession::ready("task-e", VERIFY_TIMEOUT.as_secs());
        none.copy_dir = dir.join("不存在").to_string_lossy().to_string();
        fill_params(&mut none, None, None);
        assert_eq!(none.jvm_xmx, None, "读不到就留空，界面写「未取到」");
        assert_eq!(none.online_mode, None);
        assert_eq!(none.loader_installed, None);
        assert_eq!(none.timeout_sec, VERIFY_TIMEOUT.as_secs());
        let _ = fs::remove_dir_all(&dir);
    }
}
