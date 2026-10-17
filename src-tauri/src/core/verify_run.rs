//! 本机开服验证的执行体：把交付包解成副本，按**副本自己的启动脚本**起一次服务端，认那句 `Done (`。
//! - 六步一条线：环境 → 解包 → 构建对账 → 加载器安装 → 启动服务端 → 等待就绪。前一步断在不走后一步。
//! - 只动 `{cache}\verify\{task_id}` 那份副本；交付包与任务状态表一律不写。副本里唯一会被改的
//!   配置文件是 `server.properties` 的 `server-port`——本机占用时换一个空闲端口，且日志里说清楚。
//! - `eula.txt` 没同意只在日志里提示、不挡验证（不代填 `eula=true`，那等于替用户同意 Mojang 的协议）；
//!   真起不来而失败时，这句话会同步出现在失败定位里。
//! - 启动命令抽自副本的 `start.bat`/`run.bat`：验的就是交付包会跑的那一句，只把行首的 `java`
//!   换成环境那一步解析出的绝对路径，并恒带 `nogui`（带窗口的服务端在这条链上判不出就绪）。
//! - 子进程规矩照 `core::installer`：参数 `args([...])` 不进 shell、`current_dir(副本根)`、
//!   Windows 带 `CREATE_NO_WINDOW`。就绪之后往 stdin 写一句 `stop` 让它自己退，取消/超时才 `kill()`。
//!   就绪上限沿用安装器那档 30 分钟。
//! - 纯回调（`on_event`），不认 tauri 也不认状态表：会话快照、日志环、落盘都在 `task_engine::verify`。

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crate::core::installer::{self, INSTALL_TIMEOUT};
use crate::core::{java, verify_copy};
use crate::l10n::Msg;
use crate::models::{
    CheckResult, CheckStatus, ConversionOptions, LoaderKind, LogLevel, StepStatus, VerifyFailure,
    VerifyOutputRef, VerifySource, VerifyStatus, VerifyStep, VerifyStepId, VerifyWarning,
};
use crate::msg;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 就绪等待的兜底上限：与安装器同一档（30 分钟），是「卡死了总要有人收尸」那条线，不是预算
pub const VERIFY_TIMEOUT: Duration = INSTALL_TIMEOUT;
/// 轮询步长：取消与超时最晚在此之后生效，同时决定一行输出落到界面的延迟
const POLL: Duration = Duration::from_millis(150);
/// 失败归因带出几行报错原文（沿用离线自检 `core::verify::MAX_ITEMS` 那档截断口径）
const MAX_FAILURE_LINES: usize = 8;
/// 归因里最多列几枚 jar
const MAX_JARS: usize = 8;
/// 提示（WARN）列表只留最近这些条；每一条本身也在日志环里，超了不丢事实
const MAX_WARNINGS: usize = 40;
/// 服务端准备好接客的判据：原版那句 `Done (3.456s)! For help, type "help"`，三家加载器共用
const READY_MARKER: &str = "Done (";
/// 就绪之后喂给服务端控制台的命令：让它自己收尾，这样通过档拿得到真实退出码而不是 `kill` 的结果
const STOP_COMMAND: &[u8] = b"stop\n";
/// 副本里没有 `server.properties` 时服务端要用的端口（Minecraft 的默认值，不是我们选的）
const DEFAULT_MC_PORT: u16 = 25565;

const STEP_DEFS: [(VerifyStepId, &str); 6] = [
    (VerifyStepId::Env, "检查本机环境"),
    (VerifyStepId::Unpack, "解包交付包"),
    (VerifyStepId::Reconcile, "产物对账"),
    (VerifyStepId::Loader, "首启装加载器"),
    (VerifyStepId::Launch, "启动服务端进程"),
    (VerifyStepId::Wait, "等待 Done"),
];

/// 六步的初始态：页面在还没跑过的那一档也能把这张清单列全
pub fn new_steps() -> Vec<VerifyStep> {
    STEP_DEFS
        .iter()
        .map(|(id, label)| VerifyStep {
            id: *id,
            label: (*label).to_string(),
            status: StepStatus::Pending,
            note: String::new(),
            note_msg: None,
        })
        .collect()
}

/// 一次验证的输入。`zip_path` 与 `cache_dir` 由命令层从任务/设置里取，这一层不认状态表
pub struct RunInput<'a> {
    pub cache_dir: &'a Path,
    pub task_id: &'a str,
    /// 任务产物 zip 的绝对路径。`None` = 这条任务没记下产物路径（转换没成功过），
    /// 与「记下了但文件不在了」同一个出口
    pub zip_path: Option<&'a Path>,
    pub options: &'a ConversionOptions,
    pub loader: LoaderKind,
    /// 构建后离线自检的结论（`ConversionReport.checks`）：空 = 构建时没开这个开关
    pub checks: &'a [CheckResult],
    /// 构建时已把 loader 树并进产物：目标机不再需要联网首装
    pub installed: bool,
    pub cancel: &'a AtomicBool,
    pub timeout: Duration,
}

/// 执行体向外出的一类事件
pub enum VerifyEvent {
    /// 某一步换了状态（`note` 带参数时界面按模板翻）
    Step {
        id: VerifyStepId,
        status: StepStatus,
        note: Option<Msg>,
    },
    /// 一行日志（加载器阶段的原文也从这里出）
    Line {
        source: VerifySource,
        level: LogLevel,
        msg: Msg,
    },
}

/// 一次验证的结论（会话快照由 `task_engine::verify` 据此拼）
pub struct RunResult {
    pub status: VerifyStatus,
    pub steps: Vec<VerifyStep>,
    /// 副本根；空路径 = 还没解出来
    pub copy_dir: PathBuf,
    pub files: usize,
    pub copy_bytes: u64,
    pub elapsed: Duration,
    pub exit_code: Option<i32>,
    pub server_port: Option<u16>,
    pub warnings: Vec<VerifyWarning>,
    pub failure: Option<VerifyFailure>,
    pub blockers: Vec<Msg>,
    /// 与本次失败同框的环境提示（目前只有 eula 未同意那一句）：日志里已经喊过，失败定位再摊一次
    pub hints: Vec<Msg>,
}

impl RunResult {
    fn new() -> Self {
        Self {
            status: VerifyStatus::Ready,
            steps: Vec::new(),
            copy_dir: PathBuf::new(),
            files: 0,
            copy_bytes: 0,
            elapsed: Duration::ZERO,
            exit_code: None,
            server_port: None,
            warnings: Vec::new(),
            failure: None,
            blockers: Vec::new(),
            hints: Vec::new(),
        }
    }
}

/// 事件出口：改步骤状态与播报是同一件事，分两处写迟早对不上
struct Sink<'a> {
    emit: &'a mut dyn FnMut(VerifyEvent),
}

impl<'a> Sink<'a> {
    fn step(
        &mut self,
        steps: &mut Vec<VerifyStep>,
        id: VerifyStepId,
        status: StepStatus,
        note: Option<Msg>,
    ) {
        if let Some(step) = steps.iter_mut().find(|s| s.id == id) {
            step.note = note.as_ref().map(|m| m.zh.clone()).unwrap_or_default();
            step.note_msg = note.clone();
            step.status = status;
        }
        (self.emit)(VerifyEvent::Step { id, status, note });
    }

    fn line(&mut self, source: VerifySource, level: LogLevel, msg: Msg) {
        (self.emit)(VerifyEvent::Line { source, level, msg });
    }
}

/// 每个出口都要收的三样：步表、耗时、结论
fn finish(mut res: RunResult, steps: Vec<VerifyStep>, started: Instant) -> RunResult {
    res.elapsed = started.elapsed();
    res.steps = steps;
    res
}

/// 断在哪一步：只带步骤与退出码，报错原文由 `Console` 那一本账填进 `lines`
fn failed_at(step: VerifyStepId, exit_code: Option<i32>) -> VerifyFailure {
    VerifyFailure {
        step,
        exit_code,
        jars: Vec::new(),
        lines: Vec::new(),
    }
}

pub fn run(input: &RunInput, on_event: &mut dyn FnMut(VerifyEvent)) -> RunResult {
    let started = Instant::now();
    let mut sink = Sink { emit: on_event };
    let mut steps = new_steps();
    let mut res = RunResult::new();

    // ---- 1 环境：交付包在不在、Java 够不够。协议同没同意也在这里看，但只登记不挡
    sink.step(&mut steps, VerifyStepId::Env, StepStatus::Running, None);
    let Some(zip) = input.zip_path.filter(|p| p.is_file()) else {
        let m = match input.zip_path {
            Some(p) => msg!(
                "交付包不在了：{{path}}",
                {"path": p.display().to_string()}
            ),
            None => msg!("这条任务没有记下交付包路径，先把它转换完成"),
        };
        sink.line(VerifySource::Env, LogLevel::Error, m.clone());
        sink.step(&mut steps, VerifyStepId::Env, StepStatus::Fail, Some(m));
        res.status = VerifyStatus::Missing;
        return finish(res, steps, started);
    };

    let mut blockers: Vec<Msg> = Vec::new();
    let mut eula_hint: Option<Msg> = None;
    let selected = (!input.options.java_path.trim().is_empty())
        .then(|| input.options.java_path.trim().to_string());
    let probe = java::probe(&Some(input.options.java_version.clone()), &selected);
    let have = probe.major.map(|v| v.to_string()).unwrap_or_else(|| "?".into());
    match probe.status {
        CheckStatus::Pass => {
            // 快照里那枚已经不在这台机器上了：与转换那一路同口径，只说、不挡
            if probe.selected_missing {
                sink.line(
                    VerifySource::Env,
                    LogLevel::Warn,
                    msg!("方案里指定的那枚 Java 已不在本机，改用自动挑到的那一枚"),
                );
            }
            sink.line(
                VerifySource::Env,
                LogLevel::Info,
                msg!(
                    "本次使用 Java {{have}}：{{path}}",
                    {
                        "have": have.clone(),
                        "path": probe.java_path.clone().unwrap_or_default()
                    }
                ),
            );
        }
        _ => {
            // 「没装」与「装了但低于需求线」都是不可判定：起了进程也只会让它报一句版本不够
            let m = match (probe.major, probe.required_major) {
                (Some(have), Some(need)) => msg!(
                    "本机 Java {{have}} 低于本包需要的 Java {{need}}",
                    {"have": have, "need": need}
                ),
                (None, Some(need)) => msg!(
                    "本机没有检测到 Java，本包需要 Java {{need}} 及以上",
                    {"need": need}
                ),
                _ => msg!("本机没有检测到可用的 Java"),
            };
            sink.line(VerifySource::Env, LogLevel::Error, m.clone());
            blockers.push(m);
        }
    }
    match verify_copy::eula_agreed(zip) {
        Some(true) => sink.line(
            VerifySource::Env,
            LogLevel::Info,
            msg!("交付包里 eula.txt 已同意最终用户协议"),
        ),
        other => {
            // 只提示、不挡：进程照样起得去，它自己会因为协议没同意而退出，那时归因落在那一步
            let m = if other == Some(false) {
                msg!(
                    "交付包里的 eula.txt 没有同意最终用户协议，服务端会拒绝启动（验证不代填这一项）"
                )
            } else {
                msg!("交付包里没有 eula.txt，服务端会拒绝启动")
            };
            sink.line(VerifySource::Env, LogLevel::Warn, m.clone());
            eula_hint = Some(m);
        }
    }
    if !blockers.is_empty() {
        sink.step(
            &mut steps,
            VerifyStepId::Env,
            StepStatus::Fail,
            Some(msg!("本机条件不满足，本次结果不可判定")),
        );
        res.status = VerifyStatus::Blocked;
        res.blockers = blockers;
        return finish(res, steps, started);
    }
    let java_path = PathBuf::from(probe.java_path.clone().unwrap_or_default());
    sink.step(
        &mut steps,
        VerifyStepId::Env,
        StepStatus::Pass,
        Some(msg!(
            "Java {{have}} · 交付包 {{bytes}} 字节",
            {
                "have": have,
                "bytes": std::fs::metadata(zip).map(|m| m.len()).unwrap_or_default()
            }
        )),
    );

    // ---- 2 解包：交付包整份解到副本，之后的读写都落在这份副本里
    sink.step(&mut steps, VerifyStepId::Unpack, StepStatus::Running, None);
    let prepared = match verify_copy::prepare(input.cache_dir, input.task_id, zip) {
        Ok(p) => p,
        Err(e) => {
            let m = Msg::foreign(e);
            sink.line(VerifySource::Unpack, LogLevel::Error, m.clone());
            sink.step(&mut steps, VerifyStepId::Unpack, StepStatus::Fail, Some(m));
            res.status = VerifyStatus::Failed;
            res.failure = Some(failed_at(VerifyStepId::Unpack, None));
            return finish(res, steps, started);
        }
    };
    res.copy_dir = prepared.dir.clone();
    res.files = prepared.files;
    res.copy_bytes = prepared.bytes;
    res.server_port = verify_copy::server_port(&prepared.dir);
    let note = if prepared.reused {
        msg!(
            "沿用上次副本：{{files}} 个文件 · {{bytes}} 字节",
            {"files": prepared.files, "bytes": prepared.bytes}
        )
    } else {
        msg!(
            "已解出 {{files}} 个文件 · {{bytes}} 字节",
            {"files": prepared.files, "bytes": prepared.bytes}
        )
    };
    sink.line(VerifySource::Unpack, LogLevel::Info, note.clone());
    let (mod_total, mod_disabled) = verify_copy::mod_counts(&prepared.dir);
    if mod_disabled > 0 {
        sink.line(
            VerifySource::Unpack,
            LogLevel::Warn,
            msg!(
                "mods 目录共 {{total}} 枚模组，其中 {{disabled}} 枚处于停用状态",
                {"total": mod_total, "disabled": mod_disabled}
            ),
        );
    }
    sink.step(&mut steps, VerifyStepId::Unpack, StepStatus::Pass, Some(note));

    // ---- 3 构建对账：引用构建时那份离线自检的结论，不在这里重跑（重跑要再解一遍 zip）
    sink.step(&mut steps, VerifyStepId::Reconcile, StepStatus::Running, None);
    let rec = reconcile_note(input.checks);
    let level = match rec.status {
        StepStatus::Fail => LogLevel::Error,
        StepStatus::Warn => LogLevel::Warn,
        _ => LogLevel::Info,
    };
    sink.line(VerifySource::Verify, level, rec.note.clone());
    sink.step(&mut steps, VerifyStepId::Reconcile, rec.status, Some(rec.note));

    // ---- 4 加载器：只有「构建时没本机装过」那一档才需要在副本里首启装一次
    sink.step(&mut steps, VerifyStepId::Loader, StepStatus::Running, None);
    let loader = install_loader(input, &java_path, &prepared.dir, &mut sink);
    let (loader_status, note) = match loader {
        Ok(step) => step,
        Err(m) => {
            sink.line(VerifySource::Loader, LogLevel::Error, m.clone());
            sink.step(&mut steps, VerifyStepId::Loader, StepStatus::Fail, Some(m));
            res.status = VerifyStatus::Failed;
            res.failure = Some(failed_at(VerifyStepId::Loader, None));
            return finish(res, steps, started);
        }
    };
    sink.line(VerifySource::Loader, LogLevel::Info, note.clone());
    sink.step(&mut steps, VerifyStepId::Loader, loader_status, Some(note));

    // ---- 5 启动：拿副本自己那条 java 命令，验的就是交付包会跑的那一句
    sink.step(&mut steps, VerifyStepId::Launch, StepStatus::Running, None);
    // 端口取副本配置里那一个（配置不在就是服务端自己的默认端口）。本机已经占着它就先换一个
    // 空闲的、把新值写回副本——不然进程只会因为绑定失败退出，那口锅会扣在包上
    let configured = res.server_port.unwrap_or(DEFAULT_MC_PORT);
    let rebound = ensure_free_port(&prepared.dir, configured, &mut sink);
    res.server_port = Some(rebound.unwrap_or(configured));
    let Some((script, args)) = launch_command(&prepared.dir, &java_path) else {
        let m = msg!("副本里没有以 java 开头的启动命令（start 与 run 脚本都没有）");
        sink.line(VerifySource::Java, LogLevel::Error, m.clone());
        sink.step(&mut steps, VerifyStepId::Launch, StepStatus::Fail, Some(m));
        res.status = VerifyStatus::Failed;
        res.failure = Some(failed_at(VerifyStepId::Launch, None));
        return finish(res, steps, started);
    };
    sink.line(
        VerifySource::Java,
        LogLevel::Info,
        msg!(
            "按 {{script}} 起进程：{{command}}",
            {"script": script.clone(), "command": args.join(" ")}
        ),
    );
    let mut child = match build_command(&prepared.dir, &args).spawn() {
        Ok(c) => c,
        Err(e) => {
            // 连进程都没起来（JDK 刚被删、路径不对）：报因到那一枚 java 上
            let m = msg!(
                "起进程失败：{{cause}}",
                {"cause": Msg::foreign_arg(format!("{}：{e}", java_path.display()))}
            );
            sink.line(VerifySource::Java, LogLevel::Error, m.clone());
            sink.step(&mut steps, VerifyStepId::Launch, StepStatus::Fail, Some(m));
            res.status = VerifyStatus::Failed;
            res.failure = Some(failed_at(VerifyStepId::Launch, None));
            return finish(res, steps, started);
        }
    };
    sink.step(
        &mut steps,
        VerifyStepId::Launch,
        StepStatus::Pass,
        Some(msg!("进程已起 · {{script}}", {"script": script})),
    );

    // ---- 6 等待就绪：转发输出，认那句 `Done (`
    sink.step(&mut steps, VerifyStepId::Wait, StepStatus::Running, None);
    let mut console = Console::default();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    // 控制台管道留着：就绪之后往它写一句 `stop`，服务端自己收尾才有真实退出码
    let mut stdin = child.stdin.take();
    let (tx, rx) = mpsc::channel::<Out>();
    let stop = {
        let cancel = input.cancel;
        // 两条读线程共用一个发送端：拿引用进去，`move` 才不会把 tx 搬进第一个闭包
        let tx = &tx;
        let stdin = &mut stdin;
        thread::scope(|s| {
            if let Some(out) = stdout {
                s.spawn(move || pump_out(out, tx));
            }
            if let Some(err) = stderr {
                s.spawn(move || pump_out(err, tx));
            }
            wait_server(
                &mut child,
                stdin,
                cancel,
                input.timeout,
                &rx,
                &mut sink,
                &mut console,
                started,
            )
        })
    };
    // 三种收尾都等过进程了（自己退、被取消、被超时杀掉），这里再收一次是别在长驻的应用里留僵尸
    let _ = child.wait();
    res.warnings = console.warnings;
    // 服务端起来后会重写 server.properties（补全默认项、落 level-name），端口以跑完这份为准；
    // 进程压根没起来时那份读不出新值，就保留端口闸门那一步定的数
    res.server_port = verify_copy::server_port(&prepared.dir).or(res.server_port);

    match stop {
        Stop::Ready { exit_code } => {
            let m = msg!(
                "服务端已就绪（{{sec}} 秒）",
                {"sec": started.elapsed().as_secs()}
            );
            sink.step(&mut steps, VerifyStepId::Wait, StepStatus::Pass, Some(m));
            res.status = VerifyStatus::Passed;
            res.exit_code = exit_code;
            if exit_code.is_none() {
                // 已经就绪过，收尾方式不改判；只是这一档拿不到退出码，说清楚免得界面显示成空
                sink.line(
                    VerifySource::Server,
                    LogLevel::Warn,
                    msg!("写了 stop 也没能自己退，进程已被终止（不影响通过判定，退出码记为空）"),
                );
            }
        }
        Stop::Exited { code } => {
            let m = msg!(
                "进程退出（{{code}}），没有等到就绪",
                {"code": code.map(|c| c.to_string()).unwrap_or_else(|| "无（被信号终止）".into())}
            );
            sink.step(&mut steps, VerifyStepId::Wait, StepStatus::Fail, Some(m.clone()));
            sink.line(VerifySource::Server, LogLevel::Error, m);
            res.status = VerifyStatus::Failed;
            res.exit_code = code;
            let mut f = failed_at(VerifyStepId::Wait, code);
            f.lines = console.error_lines;
            f.jars = console.jars;
            res.failure = Some(f);
            // 协议没同意正是"进程起来又自己退出"的常见原因之一，日志里喊过、这里再摊一次
            if let Some(hint) = eula_hint {
                res.hints.push(hint);
            }
        }
        Stop::TimedOut => {
            // 不判包坏：慢机器首启下载完 1.2GB 也可能过这档线，步表上标提示而不是红
            let m = msg!(
                "{{secs}} 秒内没有等到就绪，进程已终止（不判包坏）",
                {"secs": input.timeout.as_secs()}
            );
            sink.step(&mut steps, VerifyStepId::Wait, StepStatus::Warn, Some(m));
            res.status = VerifyStatus::Timeout;
        }
        Stop::Cancelled => {
            let m = msg!("已取消：服务端进程已终止，本次不作判定");
            sink.step(&mut steps, VerifyStepId::Wait, StepStatus::Warn, Some(m));
            res.status = VerifyStatus::Cancelled;
        }
        Stop::Lost => {
            let m = msg!("读不到进程状态，本次不作判定");
            sink.step(&mut steps, VerifyStepId::Wait, StepStatus::Fail, Some(m));
            res.status = VerifyStatus::Failed;
        }
    }
    finish(res, steps, started)
}

/// 构建对账那一步的结论
struct Reconcile {
    status: StepStatus,
    note: Msg,
}

fn reconcile_note(checks: &[CheckResult]) -> Reconcile {
    if checks.is_empty() {
        return Reconcile {
            status: StepStatus::Skipped,
            note: msg!("构建时没有开启自检，这一步没有依据"),
        };
    }
    let failed = checks.iter().filter(|c| c.status == CheckStatus::Fail).count();
    let warned = checks.iter().filter(|c| c.status == CheckStatus::Warn).count();
    let passed = checks.len() - failed - warned;
    let note = if failed > 0 {
        msg!(
            "构建自检有 {{failed}} 项未通过（共 {{total}} 项）",
            {"failed": failed, "total": checks.len()}
        )
    } else if warned > 0 {
        msg!(
            "构建自检 {{passed}} 项通过、{{warned}} 项提示",
            {"passed": passed, "warned": warned}
        )
    } else {
        msg!("构建自检 {{passed}} 项全部通过", {"passed": passed})
    };
    Reconcile {
        // 未通过 ≠ 起不来：这一步只搬运构建那份结论，进程照样起
        status: if failed > 0 {
            StepStatus::Fail
        } else if warned > 0 {
            StepStatus::Warn
        } else {
            StepStatus::Pass
        },
        note,
    }
}

/// 「加载器安装」这一步的走向：`Ok((步状态, 那句话))`，`Err` = 装不动
fn install_loader(
    input: &RunInput,
    java: &Path,
    copy_dir: &Path,
    sink: &mut Sink,
) -> Result<(StepStatus, Msg), Msg> {
    if input.loader == LoaderKind::Fabric {
        return Ok((
            StepStatus::Skipped,
            msg!("Fabric 的加载器随服务端首启自装，这一步不单独跑安装器"),
        ));
    }
    if input.installed {
        return Ok((
            StepStatus::Skipped,
            msg!("构建时已把加载器并进产物，副本里就是装好的那份"),
        ));
    }
    let jar = verify_copy::installer_jar_in(copy_dir)
        .ok_or_else(|| msg!("副本根目录里没有加载器安装器 jar，首启自装也无从执行"))?;
    let i = installer::InstallInput {
        java,
        installer_jar: &jar,
        dest: copy_dir,
        cancel: input.cancel,
        timeout: INSTALL_TIMEOUT,
    };
    installer::install(&i, &mut |ev| match ev {
        installer::InstallEvent::Log(m) => {
            sink.line(VerifySource::Loader, LogLevel::Info, m.clone())
        }
        // 安装器那边的文件数进度：验证页有整条日志与副本大小，这一档不重复播
        installer::InstallEvent::Progress { .. } => {}
    })
    .map(|r| {
        (
            StepStatus::Pass,
            msg!(
                "已在副本里装出 {{files}} 个文件 · {{bytes}} 字节",
                {"files": r.files, "bytes": r.bytes}
            ),
        )
    })
    .map_err(|e| e.detail_msg())
}

/// 一行输出（送进主循环前已定好色与级别）
struct Out {
    source: VerifySource,
    level: LogLevel,
    text: String,
}

/// 输出流的着色：mixin 单独一档，JVM 自己喊的那几句归 Java，其余都算服务端本体。
/// 这里**不做降噪**：验证页要看的就是控制台原文，行数由上层日志环兜住。
fn classify(text: &str) -> (VerifySource, LogLevel) {
    let lower = text.to_ascii_lowercase();
    let source = if lower.contains("mixin") {
        VerifySource::Mixin
    } else if lower.starts_with("error:")
        || lower.contains("could not find or load main class")
        || lower.contains("unrecognized option")
        || lower.contains("invalid maximum heap size")
    {
        VerifySource::Java
    } else {
        VerifySource::Server
    };
    // JVM 自己喊的那几句（`Error: Could not find or load main class`、`Unrecognized option`…）
    // 没有加载器那种 `[线程/级别]` 前缀，但它一律是致命的 ⇒ 跟着着色一档走
    let level = if source == VerifySource::Java
        || lower.contains("/error]")
        || lower.contains("exception")
        || lower.starts_with("caused by:")
    {
        LogLevel::Error
    } else if lower.contains("/warn]") || lower.contains("warning") {
        LogLevel::Warn
    } else {
        LogLevel::Info
    };
    (source, level)
}

/// 一条管道的读循环：非空行一律转发，进程一死管道关闭即退出（`scope` 的 join 因此不挂住）
fn pump_out<R: Read>(src: R, tx: &mpsc::Sender<Out>) {
    let mut reader = BufReader::new(src);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            // 读不动就到此为止：剩下的信息在退出码里，不缺这一行
            Err(_) => break,
        }
        let text = line.trim().to_string();
        if text.is_empty() {
            continue;
        }
        let (source, level) = classify(&text);
        let _ = tx.send(Out {
            source,
            level,
            text,
        });
    }
}

/// 本次输出流的账：行数、提示列表、报错原文与点到的 jar
#[derive(Default)]
struct Console {
    lines: u32,
    warnings: Vec<VerifyWarning>,
    error_lines: Vec<VerifyOutputRef>,
    jars: Vec<String>,
}

impl Console {
    /// 收一行：转发给界面，并记账。返回是不是那句就绪
    fn absorb(&mut self, out: &Out, sink: &mut Sink) -> bool {
        self.lines += 1;
        let seq = self.lines;
        sink.line(out.source, out.level, Msg::foreign(out.text.clone()));
        if out.level == LogLevel::Warn {
            if self.warnings.len() >= MAX_WARNINGS {
                self.warnings.remove(0);
            }
            self.warnings.push(VerifyOutputRef {
                line: seq,
                text: out.text.clone(),
            });
        }
        if out.level == LogLevel::Error {
            if self.error_lines.len() >= MAX_FAILURE_LINES {
                self.error_lines.remove(0);
            }
            self.error_lines.push(VerifyOutputRef {
                line: seq,
                text: out.text.clone(),
            });
            for jar in jars_in(&out.text) {
                if self.jars.len() < MAX_JARS && !self.jars.contains(&jar) {
                    self.jars.push(jar);
                }
            }
        }
        out.text.contains(READY_MARKER)
    }
}

/// 一行里点到的 jar 名：只留文件名、去重
fn jars_in(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for token in text.split(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '[' | ']')) {
        let name = token.rsplit(['/', '\\']).next().unwrap_or(token);
        if name.len() > 4 && name.to_ascii_lowercase().ends_with(".jar") && !out.contains(&name.to_string()) {
            out.push(name.to_string());
        }
    }
    out
}

enum Stop {
    /// 读到就绪句：`exit_code` 是喂了 `stop` 之后它自己退的那个数，没等到为 `None`
    Ready { exit_code: Option<i32> },
    Exited { code: Option<i32> },
    Cancelled,
    TimedOut,
    /// `try_wait` 都读不动（不该发生，但起了进程就不能假装它还在）
    Lost,
}

/// 服务端跑着时的主循环：随到随转发，就绪 / 退出 / 取消 / 超时 四选一
fn wait_server(
    child: &mut Child,
    stdin: &mut Option<ChildStdin>,
    cancel: &AtomicBool,
    timeout: Duration,
    rx: &mpsc::Receiver<Out>,
    sink: &mut Sink,
    console: &mut Console,
    started: Instant,
) -> Stop {
    loop {
        while let Ok(out) = rx.try_recv() {
            if console.absorb(&out, sink) {
                let code = graceful_stop(child, stdin, cancel, rx, sink, console, timeout, started);
                return Stop::Ready { exit_code: code };
            }
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                // 进程已停 ⇒ 管道随之关闭、读线程马上退出：通道里剩下的行这一轮都能取到
                while let Ok(out) = rx.try_recv() {
                    console.absorb(&out, sink);
                }
                return Stop::Exited {
                    code: status.code(),
                };
            }
            Ok(None) => {}
            Err(_) => {
                let _ = child.kill();
                return Stop::Lost;
            }
        }
        if cancel.load(Ordering::Relaxed) {
            // 先杀再退：进程一死两条读线程才拿得到 EOF，scope 的 join 不会悬着
            let _ = child.kill();
            return Stop::Cancelled;
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            return Stop::TimedOut;
        }
        thread::sleep(POLL);
    }
}

/// 就绪之后让它自己收摊：往控制台管道写一句 `stop`，再把剩下的输出继续转发到进程停。
/// 拿到退出码就带回去；到兜底上限还在跑（或中途被要求取消）才 `kill()`，那种情况带回 `None`。
/// 判定不因它变——就绪已经发生过，这一档只是不留僵尸进程。
fn graceful_stop(
    child: &mut Child,
    stdin: &mut Option<ChildStdin>,
    cancel: &AtomicBool,
    rx: &mpsc::Receiver<Out>,
    sink: &mut Sink,
    console: &mut Console,
    timeout: Duration,
    started: Instant,
) -> Option<i32> {
    if let Some(io) = stdin.as_mut() {
        // 写不进去（服务端没接控制台）也不改判：下面等不到退出码会走 kill 兜底
        let _ = io.write_all(STOP_COMMAND).and_then(|_| io.flush());
    }
    // 关掉这一路：读到 EOF 的服务端会自己收尾，握着句柄会让它一直等输入
    *stdin = None;
    loop {
        while let Ok(out) = rx.try_recv() {
            // 不再判就绪句，只转发：`stop` 之后那几行（保存世界、Closing session）正是这段的收尾
            console.absorb(&out, sink);
        }
        match child.try_wait() {
            Ok(Some(status)) => return status.code(),
            Ok(None) => {}
            Err(_) => {
                let _ = child.kill();
                return None;
            }
        }
        if cancel.load(Ordering::Relaxed) || started.elapsed() >= timeout {
            let _ = child.kill();
            return None;
        }
        thread::sleep(POLL);
    }
}

/// 端口闸门：副本配置那个端口本机已经占了 ⇒ 换一个空闲的、写回副本配置，日志里说清楚。
/// 返回 `Some(新端口)` = 换了；`None` = 不用换，或换不成（问不到空闲端口、副本写不回去）——
/// 换不成时留在原端口起进程，报出来的至少是包自己的错，好过把副本配置写坏
fn ensure_free_port(copy_dir: &Path, configured: u16, sink: &mut Sink) -> Option<u16> {
    if !verify_copy::port_in_use(configured) {
        return None;
    }
    let next = verify_copy::free_port()?;
    match verify_copy::set_server_port(copy_dir, next) {
        Ok(()) => {
            sink.line(
                VerifySource::Env,
                LogLevel::Warn,
                msg!(
                    "端口 {{from}} 已被本机其它程序占用，本次改用空闲端口 {{to}}：新值只写进副本的 server.properties，交付包不动",
                    {"from": configured, "to": next}
                ),
            );
            Some(next)
        }
        Err(e) => {
            sink.line(
                VerifySource::Env,
                LogLevel::Warn,
                msg!(
                    "端口 {{from}} 已被占用，空闲端口 {{to}} 写不回副本，仍用原端口起进程：{{cause}}",
                    {
                        "from": configured,
                        "to": next,
                        "cause": Msg::foreign_arg(e)
                    }
                ),
            );
            None
        }
    }
}

/// 副本里的启动命令：优先 `start.*`，它转调 `run.*`（首启自装那一档）就退回去读 `run.*`。
/// 行首的 `java` 换成 `java_path`（脚本里写的是裸命令， PATH 上那枚未必满足需求线），
/// 参数原样保留并恒补 `nogui`。
fn launch_command(copy_dir: &Path, java: &Path) -> Option<(String, Vec<String>)> {
    #[cfg(windows)]
    const CANDIDATES: [&str; 2] = ["start.bat", "run.bat"];
    #[cfg(not(windows))]
    const CANDIDATES: [&str; 2] = ["start.sh", "run.sh"];

    for name in CANDIDATES {
        let path = copy_dir.join(name);
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Some(args) = java_args(&text) else {
            continue;
        };
        let mut full = vec![java.to_string_lossy().to_string()];
        full.extend(args);
        if !full.iter().any(|a| a == "nogui") {
            full.push("nogui".to_string());
        }
        return Some((name.to_string(), full));
    }
    None
}

/// 脚本里那条以 `java` 开头的命令行，抽参数（不含命令本身）
fn java_args(script: &str) -> Option<Vec<String>> {
    for raw in script.lines() {
        let line = raw.trim();
        let Some(rest) = line.strip_prefix("java") else {
            continue;
        };
        if !(rest.is_empty() || rest.starts_with(' ') || rest.starts_with('\t')) {
            continue;
        }
        // 首启自装那档的 start 脚本里也有一句 java（`java -jar x-installer.jar --installServer`），
        // 但它挂在 `if not exist run.bat` 之后、不是行首 ⇒ 这里天然不命中，转去读 run 脚本
        let args: Vec<String> = rest
            .split_whitespace()
            .map(str::to_string)
            .filter(|t| !matches!(t.as_str(), "%*" | "$@" | "\"$@\"" | "&" | "pause"))
            .collect();
        if !args.is_empty() {
            return Some(args);
        }
    }
    None
}

fn build_command(dest: &Path, args: &[String]) -> Command {
    let mut cmd = Command::new(&args[0]);
    cmd.args(&args[1..])
        .current_dir(dest)
        // 控制台管道：就绪之后那句 `stop` 从这一路喂进去
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 四态启动脚本都要抽出对的那条命令，且剥掉转传与 pause
    #[test]
    fn extracts_java_args_from_every_script_shape() {
        // 已装好的新式：靠参数文件启动
        assert_eq!(
            java_args("@echo off\r\ncd /d \"%~dp0\"\r\njava @user_jvm_args.txt @libraries/net/neoforged/neoforge/26.2.1.1-win_args.txt %*\r\npause\r\n"),
            Some(vec![
                "@user_jvm_args.txt".to_string(),
                "@libraries/net/neoforged/neoforge/26.2.1.1-win_args.txt".to_string()
            ])
        );
        // 老 Forge：单 jar + 内存参数
        assert_eq!(
            java_args("@echo off\r\njava -Xmx4G -jar forge-1.16.5-36.2.39.jar nogui\r\npause\r\n"),
            Some(vec![
                "-Xmx4G".to_string(),
                "-jar".to_string(),
                "forge-1.16.5-36.2.39.jar".to_string(),
                "nogui".to_string()
            ])
        );
        // 首启自装：那句 java 不在行首 ⇒ 这份脚本认不出，由 launch_command 回落到 run.bat
        assert_eq!(
            java_args(
                "@echo off\r\ncd /d \"%~dp0\"\r\nif not exist run.bat java -Xmx1G -jar forge-installer.jar --installServer\r\nif exist run.bat (call run.bat nogui) else (echo x & pause)\r\n"
            ),
            None
        );
        // Fabric 服务端 jar（sh 那份同样形状）
        assert_eq!(
            java_args("#!/usr/bin/env bash\ncd \"$(dirname \"$0\")\"\njava -Xmx4G -jar fabric-server-launch.jar nogui\n"),
            Some(vec![
                "-Xmx4G".to_string(),
                "-jar".to_string(),
                "fabric-server-launch.jar".to_string(),
                "nogui".to_string()
            ])
        );
    }

    /// 行里的 jar 名只留文件名、去重；就绪句里没有 jar
    #[test]
    fn picks_jar_names_out_of_error_lines() {
        assert_eq!(
            jars_in("Caused by: java.lang.NoClassDefFoundError: at /libs/sodium-0.5.8.jar[x]"),
            vec!["sodium-0.5.8.jar".to_string()]
        );
        assert!(jars_in("Done (3.1s)!").is_empty());
    }

    /// 着色与级别：mixin 单独一档，JVM 那句归 Java，其余算服务端
    #[test]
    fn classifies_server_output() {
        assert_eq!(
            classify("[main/WARN] [mixin/]: ref target not found").0,
            VerifySource::Mixin
        );
        assert_eq!(
            classify("[main/WARN] [mixin/]: x").1,
            LogLevel::Warn
        );
        assert_eq!(
            classify("Error: Could not find or load main class net.fabricmc.loader").0,
            VerifySource::Java
        );
        assert_eq!(
            classify("Error: Could not find or load main class x").1,
            LogLevel::Error
        );
        assert_eq!(
            classify("[Server thread/INFO]: Done (4.2s)! For help").0,
            VerifySource::Server
        );
        assert_eq!(classify("[Server thread/ERROR]: x").1, LogLevel::Error);
        assert_eq!(classify("[Server thread/INFO]: x").1, LogLevel::Info);
    }

    /// 六步初始态：顺序与 id 固定，全是「未执行」
    #[test]
    fn steps_start_pending_in_order() {
        let steps = new_steps();
        assert_eq!(steps.len(), 6);
        assert_eq!(steps[0].id, VerifyStepId::Env);
        assert_eq!(steps[5].id, VerifyStepId::Wait);
        assert!(steps.iter().all(|s| s.status == StepStatus::Pending));
    }
}
