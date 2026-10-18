use super::*;
use crate::core::installer_deps;
use crate::l10n::Msg;
use crate::msg;

/// 开了「本机安装 Loader」⇒ 官方安装器 jar 从主轮计划里拎出来提前取（阶段 2.5），
/// 关着 ⇒ 一切照旧，它仍是取件计划里的一项。判据只在这一个函数里，两家不会走偏
pub(super) fn push_loader_item(
    install: bool,
    items: &mut Vec<ItemSpec>,
    first: &mut Option<ItemSpec>,
    spec: ItemSpec,
) {
    if install {
        *first = Some(spec);
    } else {
        items.push(spec);
    }
}

/// 单独取一枚加载器 jar（本机安装的前置件）。走同一个 `download_all`，所以重试、sha1 校验、
/// 下载缓存那套口径与主轮完全一致。实时条用**它自己的**账本（`items_total = 1`）：
/// 拿整包的计划量当分母会把「十几 MB 的安装器」画成「整包下好了 5%」。
pub(super) async fn prefetch_loader_jar(
    app: &AppHandle,
    state: &Arc<AppState>,
    id: &str,
    dl: &mut Downloader,
    cancel: &Arc<AtomicBool>,
    spec: ItemSpec,
) -> Result<(), DownloadError> {
    let agg = Arc::new(Mutex::new(NetActivity::new(spec.size_bytes, 1)));
    let (app_t, state_t, id_t) = (app.clone(), state.clone(), id.to_string());
    let (agg_t, cancel_t) = (agg.clone(), cancel.clone());
    dl.set_transfer(Arc::new(move |p: &TransferProgress| {
        if cancel_t.load(Ordering::Relaxed) {
            return;
        }
        // 锁顺序照旧：先账本、后任务表
        if let Some(info) = agg_t.lock().unwrap().record(p, 0) {
            update(&app_t, &state_t, &id_t, |t| t.activity = Some(info), true);
        }
    }));
    let (app_f, state_f, id_f) = (app.clone(), state.clone(), id.to_string());
    let cancel_f = cancel.clone();
    let res = dl
        .download_all(vec![spec], cancel.clone(), move |done, tot, oc| {
            if cancel_f.load(Ordering::Relaxed) || done < tot {
                return;
            }
            // 一项的爬坡没有意义（只有落位那一刻），总条交给 30→34 这一步，
            // 途中那十几 MB 由实时条按真实字节走
            let _ = done;
            let (a, s, i) = (app_f.clone(), state_f.clone(), id_f.clone());
            let line = msg!(
                "安装器就位：{{fileName}} · {{size}}",
                {"fileName": oc.file_name.clone(), "size": fmt_size(oc.bytes)}
            );
            log_update(
                &a,
                &s,
                &i,
                PipelineStage::Downloader,
                if oc.retries > 0 { LogLevel::Warn } else { LogLevel::Info },
                line,
                move |t| t.progress = t.progress.max(LOADER_JAR_TO),
            );
        })
        .await;
    if res.is_ok() {
        // 预取的这一项不属于主轮账本，实时条到此收摊
        update(app, state, id, |t| t.activity = None, true);
    }
    res
}

/// 阶段 2.5 的入参：一次本机安装需要知道的全部坐标（都在任务快照里，不回头读全局设置）
pub(super) struct InstallStage<'a> {
    pub(super) loader: LoaderKind,
    pub(super) mc_version: &'a str,
    pub(super) loader_version: &'a str,
    /// 本次转换的 Java 需求线（`options.java_version`）：只当筛子，决定"够不够"，不决定用哪一枚
    pub(super) java_required: &'a str,
    /// 用户在转换页手选的那枚 JDK 绝对路径（`options.java_path`）；空串 = 自动挑"够格且最低"的那一枚
    pub(super) java_selected: &'a str,
    /// 设置里那颗「复用已装的 Loader」
    pub(super) reuse: bool,
    pub(super) cache_dir: &'a Path,
    /// 阶段 2.5 单独取到 staging 的官方 installer jar
    pub(super) installer_jar: &'a Path,
    /// reuse 关时的落点（任务私有目录）
    pub(super) scratch: &'a Path,
    pub(super) cancel: &'a Arc<AtomicBool>,
}

/// 本机安装没走到末态的两种收场：取消不算失败（取消是用户意图，不该出错误卡）
pub(super) enum InstallStop {
    Cancelled,
    Failed(TaskError),
}

/// 在本机跑 loader 官方安装器，拿到一份可用的 loader 树（进度、日志、失败卡都在这段外显）。
///
/// 四条口径：① **决定①——跑不成即任务失败**，不静默退回「产物到服务器上首启自装」那条老路
/// （那种包在国内服务器上最常卡住，而用户以为已经装好了）；② 装的那一段放在 `spawn_blocking`：
/// 一趟实测 3~4 分钟，跑在 async 线程上会把其他任务的取消都堵住；③ Java 探测排在取数之前——
/// 可预见的失败不该先花一趟几百 MB；④ 安装依赖由我们预置（`seed_install_deps`），预置那一条腿
/// 断了不当失败，安装器自己会补没取到的那几枚。
/// `dl` 是主轮那台下载器（档位、缓存、并发都已挂好）：预置走它，子进程才一发请求都不用发。
pub(super) async fn run_installer_stage(
    app: &AppHandle,
    state: &Arc<AppState>,
    id: &str,
    stage: InstallStage<'_>,
    dl: &mut Downloader,
) -> Result<Installed, InstallStop> {
    update(app, state, id, |t| {
        t.stage = Some(PipelineStage::Installer);
        t.progress = INSTALL_FROM;
        t.activity = None;
    }, true);

    let (a, s, i) = (app.clone(), state.clone(), id.to_string());
    // 实时条与日志都认这一个名字：安装器没有「正在第几个包」的接口，主体只能说到这一档为止
    let subject = format!(
        "{} {}-{}",
        stage.loader.as_label(),
        stage.mc_version,
        stage.loader_version
    );

    // 探 Java 排在取数之前：「本机没有可用的 Java」这种失败不该先花一趟几百 MB
    let java = locate_java(
        &a,
        &s,
        &i,
        &subject,
        stage.java_required,
        stage.java_selected,
    )
    .await?;
    // 预置安装依赖 ⇒ 子进程那侧的境外请求归零（它既不认下载源、也不认系统代理）
    let pre_seeded = seed_install_deps(&a, &s, &i, dl, &stage).await?;

    // 取数已经落位，剩下的坐标拷成 owned 交给阻塞线程（`InstallStage` 的字段全是 `Copy`）
    let (loader, reuse) = (stage.loader, stage.reuse);
    let (mc, ver) = (
        stage.mc_version.to_string(),
        stage.loader_version.to_string(),
    );
    let (cache, jar, scratch_dir) = (
        stage.cache_dir.to_path_buf(),
        stage.installer_jar.to_path_buf(),
        stage.scratch.to_path_buf(),
    );
    let cancel_flag = stage.cancel.clone();

    let joined = tokio::task::spawn_blocking(move || -> Result<Installed, InstallStop> {
        let started = Instant::now();
        let input = installer::EnsureInput {
            loader,
            mc_version: &mc,
            loader_version: &ver,
            reuse,
            cache_dir: &cache,
            scratch: &scratch_dir,
            java: &java,
            installer_jar: &jar,
            cancel: &cancel_flag,
            timeout: installer::INSTALL_TIMEOUT,
            pre_seeded,
        };
        // 安装器侧每 500ms 才扫一次目录，节拍已经比取件慢一个量级，不再另攒窗口
        let mut on_event = |ev: InstallEvent| match ev {
            InstallEvent::Log(line) => push_log(
                &a,
                &s,
                &i,
                PipelineStage::Installer,
                LogLevel::Info,
                // 里程碑行自带 `msg!` 模板；安装器喊的报错行是 `Msg::foreign`，
                // 界面按形状整成「对象 · 病因」（见 `src/lib/log-normalize.ts`）
                line.clone(),
            ),
            InstallEvent::Progress { files, bytes } => {
                let secs = started.elapsed().as_secs_f64();
                let info = ActivityInfo {
                    kind: ActivityKind::Install,
                    subject: subject.clone(),
                    done_bytes: bytes,
                    // 总量未知（安装器不报总量）：实时条据此走不定态，不假装快满了
                    total_bytes: 0,
                    items_done: files.min(u32::MAX as u64) as u32,
                    items_total: 0,
                    rate_bps: if secs > 0.0 { bytes as f64 / secs } else { 0.0 },
                    attempt: 1,
                };
                update(
                    &a,
                    &s,
                    &i,
                    move |t| {
                        t.activity = Some(info);
                        // 总条只按估算爬坡，且只往上走：复用命中那一拍直接给末态数字
                        let p = install_progress(bytes);
                        if p > t.progress {
                            t.progress = p;
                        }
                    },
                    true,
                );
            }
        };

        match installer::ensure(&input, &mut on_event) {
            Ok(v) => {
                push_log(
                    &a,
                    &s,
                    &i,
                    PipelineStage::Installer,
                    LogLevel::Info,
                    install_done_line(&v, &subject),
                );
                // 落点要说出来：复用模式下这里就是缓存桶的坐标，用户排查「装到哪去了」只靠这一行
                push_log(
                    &a,
                    &s,
                    &i,
                    PipelineStage::Installer,
                    LogLevel::Info,
                    msg!("安装目录 {{path}}", {"path": v.dir.display().to_string()}),
                );
                update(&a, &s, &i, |t| t.progress = INSTALL_TO, true);
                Ok(v)
            }
            Err(installer::InstallError::Cancelled) => Err(InstallStop::Cancelled),
            Err(e) => Err(InstallStop::Failed(install_task_error(&e))),
        }
    })
    .await;

    match joined {
        Ok(r) => r,
        // 阻塞线程本身炸了（内部 panic）：与构建侧同款口径报失败，不让人对着一个不动的进度猜
        Err(e) => Err(thread_crash(&e)),
    }
}

/// 阻塞线程异常 → 失败卡（`spawn_blocking` 的两个出口共用一条说法）
fn thread_crash(e: &tokio::task::JoinError) -> InstallStop {
    let detail = msg!("安装线程异常：{{cause}}", {"cause": Msg::foreign_arg(e.to_string())});
    InstallStop::Failed(TaskError {
        stage: PipelineStage::Installer,
        title: "本机安装异常".into(),
        detail_msg: Some(detail.clone()),
        detail: detail.zh,
        code: None,
        retryable: true,
        attempts: None,
        log_tail: None,
        exit_code: None,
    })
}

/// 探一枚本机 JDK，并把开场那两行说掉。`java::probe` 自己会起 `java -version`，是阻塞调用，
/// 所以单开一次 `spawn_blocking`——它排在取数之前，为的是「本机没 Java」别先花一趟几百 MB。
async fn locate_java(
    a: &AppHandle,
    s: &Arc<AppState>,
    i: &str,
    subject: &str,
    java_required: &str,
    java_selected: &str,
) -> Result<PathBuf, InstallStop> {
    let (req, sel) = (java_required.to_string(), java_selected.trim().to_string());
    let probe = tokio::task::spawn_blocking(move || java::probe(&Some(req), &Some(sel)))
        .await
        .map_err(|e| thread_crash(&e))?;
    let Some(found) = probe.java_path else {
        return Err(InstallStop::Failed(TaskError {
            stage: PipelineStage::Installer,
            title: "本机没有可用的 Java".into(),
            detail: probe.detail,
            detail_msg: None,
            code: None,
            retryable: true,
            attempts: None,
            log_tail: None,
            exit_code: None,
        }));
    };
    // 快照里那枚已经不在这台机器上了（卸载、换盘符、换机）：退回自动那一枚继续跑，
    // 但要把换过说在日志里 —— 不然报告写着 Java 21、日志用的却是另一枚，查起来两头对不上
    if probe.selected_missing {
        push_log(
            a,
            s,
            i,
            PipelineStage::Installer,
            LogLevel::Warn,
            msg!("方案里指定的那枚 Java 已不在本机，改用自动挑到的那一枚"),
        );
    }
    push_log(
        a,
        s,
        i,
        PipelineStage::Installer,
        LogLevel::Info,
        msg!(
            "本机安装 {{subject}} · Java {{java}} · {{path}}",
            {
                "subject": subject,
                "java": probe.major.map(|m| m.to_string()).unwrap_or_else(|| "?".into()),
                "path": found.clone()
            }
        ),
    );
    Ok(PathBuf::from(&found))
}

/// 起子进程之前，把安装器要去远程取的那些文件先落进 `{工作目录}/libraries/`。
///
/// 值得单开这一趟的理由只有一条：那些请求本来出自 Java 子进程，它既不认设置里的下载源、也不认
/// 系统代理，而 `maven.neoforged.net` 在简中网络是被 DNS 投毒的（实测预置 135 枚后子进程境外请求
/// 归零、退出码 0）。交给我们自己的下载器就同时拿到镜像档位、回落、sha1、缓存与重试。
/// 落位后安装器认「文件已在且校验通过 ⇒ 跳过下载」，所以它一个字节也不会重复取。
///
/// 三条收口口径：① 复用命中时直接跳过（那一趟连进程都不起，预置就是白下几百 MB）；
/// ② 清单为空（读不出 profile 的形状）也说跳过，不解释成失败；
/// ③ **取数失败不当失败**：Warn 一句继续起子进程，没取到的那几枚安装器照旧自己取 ⇒
/// 这条腿只可能让事情变好。返回的 `bool` = 「工作目录已经清过（并按需写进去过）」，
/// `ensure` 据此不再扫一遍——扫一下就等于把刚下载好的库删掉。
async fn seed_install_deps(
    a: &AppHandle,
    s: &Arc<AppState>,
    i: &str,
    dl: &mut Downloader,
    stage: &InstallStage<'_>,
) -> Result<bool, InstallStop> {
    let (loader, reuse) = (stage.loader, stage.reuse);
    if reuse && installer::reuse_hit(stage.cache_dir, loader, stage.mc_version, stage.loader_version) {
        return Ok(false);
    }
    let deps = installer_deps::list(stage.installer_jar);
    if deps.is_empty() {
        return Ok(false);
    }
    let work = installer::work_dir(
        stage.cache_dir,
        loader,
        stage.mc_version,
        stage.loader_version,
        reuse,
        stage.scratch,
    );
    // 上一次被强杀留下的残骸先清掉：留着会让「已在且校验通过」的判据去认旧内容
    if let Err(e) = installer::clear_work(&work) {
        push_log(
            a,
            s,
            i,
            PipelineStage::Installer,
            LogLevel::Warn,
            msg!(
                "预置目录没清干净，本次跳过预置：{{cause}}",
                {"cause": Msg::foreign_arg(e.to_string())}
            ),
        );
        return Ok(false);
    }

    let planned: u64 = deps.iter().map(|d| d.size_bytes).sum();
    let items: Vec<ItemSpec> = deps
        .iter()
        .map(|d| ItemSpec {
            fetch: Fetch::Url(d.url.clone()),
            file_name: d.rel.rsplit('/').next().unwrap_or(&d.rel).to_string(),
            sha1: d.sha1.clone(),
            // 安装器拿工作目录当安装位置，库的相对位置是 maven 那套（`{work}/libraries/<path>`）
            dest: work.join("libraries").join(&d.rel),
            size_bytes: d.size_bytes,
        })
        .collect();
    let count = items.len() as u32;
    push_log(
        a,
        s,
        i,
        PipelineStage::Installer,
        LogLevel::Info,
        msg!(
            "预置安装依赖：{{files}} 个文件 · {{size}}",
            {"files": count, "size": fmt_size(planned)}
        ),
    );

    // 账本用这批文件自己的量（与预取安装器 jar 同款口径）：拿整包计划量当分母会把这条画歪
    let agg = Arc::new(Mutex::new(NetActivity::new(planned, count)));
    let net = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let (app_t, state_t, id_t) = (a.clone(), s.clone(), i.to_string());
    let (agg_t, net_t, cancel_t) = (agg.clone(), net.clone(), stage.cancel.clone());
    dl.set_transfer(Arc::new(move |p: &TransferProgress| {
        if cancel_t.load(Ordering::Relaxed) {
            return;
        }
        // 锁顺序照旧：先账本、后任务表。总条按已收字节在安装档里爬坡，与子进程那一段接得上
        if let Some(info) = agg_t.lock().unwrap().record(p, net_t.load(Ordering::Relaxed)) {
            let climb = install_progress(info.done_bytes);
            update(
                &app_t,
                &state_t,
                &id_t,
                move |t| {
                    t.activity = Some(info);
                    if climb > t.progress {
                        t.progress = climb;
                    }
                },
                true,
            );
        }
    }));
    let started = Instant::now();
    let net_f = net.clone();
    let res = dl
        .download_all(items, stage.cancel.clone(), move |_, _, oc| {
            // 只数「真联网且没用上缓存」的那些：与主轮同一口径，缓存命中不该占实时条的条数
            if matches!(oc.source, FetchSource::Network) && !oc.cached {
                net_f.fetch_add(1, Ordering::Relaxed);
            }
        })
        .await;
    // 预置这一段不属于安装器自己那趟，实时条到此收摊
    update(a, s, i, |t| t.activity = None, true);
    if stage.cancel.load(Ordering::Relaxed) {
        return Err(InstallStop::Cancelled);
    }

    match res {
        Ok(()) => {
            push_log(
                a,
                s,
                i,
                PipelineStage::Installer,
                LogLevel::Info,
                msg!(
                    "安装依赖已就位：{{files}} 个文件 · 用时 {{secs}} 秒",
                    {"files": count, "secs": format!("{:.1}", started.elapsed().as_secs_f64())}
                ),
            );
        }
        // `download_all` 一条失败即叫停其余（避免几百次白落盘），所以这里说的是「中断」不是「少了几枚」
        Err(e) => {
            push_log(
                a,
                s,
                i,
                PipelineStage::Installer,
                LogLevel::Warn,
                msg!(
                    "安装依赖预置中断：{{cause}}（没取到的那些由安装器自己取）",
                    {"cause": Msg::foreign_arg(e.to_string())}
                ),
            );
        }
    }
    Ok(true)
}

/// 安装成功的收场行：命中复用与真装出来的说法必须分开——前者一次进程都没起。
/// 顶层布局也写进来：第 6 步的启动脚本三分叉就是按「有没有 run 脚本 / 顶层是不是散 jar」判的，
/// 现在让它先在日志里可见，装错了能在这一行看出来
pub(super) fn install_done_line(installed: &Installed, subject: &str) -> Msg {
    let counts = msg!(
        "{{files}} 个文件 · {{size}}",
        {"files": installed.report.files, "size": fmt_size(installed.report.bytes)}
    );
    let layout = describe_layout(&installed.report.scripts, &installed.report.jars);
    // 数量与布局各是一句「模板 + 参数」，嵌进外层当参数：拍平成字符串就永远是中文
    if installed.from_cache {
        msg!(
            "复用已装的 {{subject}} · {{counts}} · 未起进程 · {{layout}}",
            {
                "subject": subject,
                "counts": counts.arg(),
                "layout": layout.arg()
            }
        )
    } else {
        msg!(
            "本机安装完成 · {{subject}} · {{counts}} · 用时 {{secs}} 秒 · {{layout}}",
            {
                "subject": subject,
                "counts": counts.arg(),
                "secs": format!("{:.1}", installed.report.elapsed.as_secs_f64()),
                "layout": layout.arg()
            }
        )
    }
}

pub(super) fn describe_layout(scripts: &[String], jars: &[String]) -> Msg {
    let head = if scripts.is_empty() { None } else { Some(scripts.join("、")) };
    let tail = if jars.is_empty() { None } else { Some(jars.join("、")) };
    match (head, tail) {
        (Some(s), None) => msg!("顶层 {{scripts}}", {"scripts": s}),
        (Some(s), Some(j)) => msg!("顶层 {{scripts}} + {{jars}}", {"scripts": s, "jars": j}),
        (None, Some(j)) => msg!("无 run 脚本，顶层散 jar {{jars}}", {"jars": j}),
        (None, None) => msg!("顶层既无 run 脚本也无散 jar"),
    }
}

/// 安装失败 → 错误卡载荷。退出码在安装器那边是字符串（Windows 与 Unix 口径不同），
/// 留在 detail 里说，不硬塞进 `TaskError.exit_code` 那个 i32 槽
pub(super) fn install_task_error(e: &installer::InstallError) -> TaskError {
    let title = match e {
        installer::InstallError::Io(_) => "安装目录准备失败",
        installer::InstallError::Spawn(_) => "Java 起不来",
        installer::InstallError::Timeout { .. } => "本机安装超时",
        installer::InstallError::Failed { .. } => "安装器报错",
        installer::InstallError::Incomplete { .. } => "安装器报成功却没装出结果",
        installer::InstallError::Cancelled => "本机安装已取消",
    };
    // 明细的中文整句与「模板 + 参数」同源于 `InstallError::detail_msg`（那边有一条单测钉住），
    // 这里不再自己拼尾巴
    let detail_msg = e.detail_msg();
    TaskError {
        stage: PipelineStage::Installer,
        title: title.into(),
        detail_msg: Some(detail_msg.clone()),
        detail: detail_msg.zh,
        code: None,
        retryable: true,
        attempts: None,
        log_tail: None,
        exit_code: None,
    }
}
