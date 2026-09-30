use super::*;

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
            let line = format!("安装器就位：{} · {}", oc.file_name, fmt_size(oc.bytes));
            log_update(
                &a,
                &s,
                &i,
                PipelineStage::Downloader,
                if oc.retries > 0 { LogLevel::Warn } else { LogLevel::Info },
                &line,
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
/// 三条口径：① **决定①——跑不成即任务失败**，不静默退回「产物到服务器上首启自装」那条老路
/// （那种包在国内服务器上最常卡住，而用户以为已经装好了）；② 整段放在 `spawn_blocking`：
/// 一趟实测 3~4 分钟，跑在 async 线程上会把其他任务的取消都堵住；③ Java 探测也在阻塞线程里
/// 现探（它自己起 `java -version`），且排在子进程之前——可预见的失败不该等一趟几分钟的安装。
pub(super) async fn run_installer_stage(
    app: &AppHandle,
    state: &Arc<AppState>,
    id: &str,
    stage: InstallStage<'_>,
) -> Result<Installed, InstallStop> {
    update(app, state, id, |t| {
        t.stage = Some(PipelineStage::Installer);
        t.progress = INSTALL_FROM;
        t.activity = None;
    }, true);

    let InstallStage {
        loader,
        mc_version,
        loader_version,
        java_required,
        java_selected,
        reuse,
        cache_dir,
        installer_jar,
        scratch,
        cancel,
    } = stage;
    let (a, s, i) = (app.clone(), state.clone(), id.to_string());
    let (mc, ver, java_req, java_sel) = (
        mc_version.to_string(),
        loader_version.to_string(),
        java_required.to_string(),
        java_selected.trim().to_string(),
    );
    let (cache, jar, scratch_dir) = (cache_dir.to_path_buf(), installer_jar.to_path_buf(), scratch.to_path_buf());
    let cancel_flag = cancel.clone();
    // 实时条与日志都认这一个名字：安装器没有「正在第几个包」的接口，主体只能说到这一档为止
    let subject = format!("{} {}-{}", loader.as_label(), mc, ver);

    let joined = tokio::task::spawn_blocking(move || -> Result<Installed, InstallStop> {
        let probe = java::probe(&Some(java_req), &Some(java_sel));
        let Some(found) = probe.java_path else {
            return Err(InstallStop::Failed(TaskError {
                stage: PipelineStage::Installer,
                title: "本机没有可用的 Java".into(),
                detail: probe.detail,
                code: None,
                retryable: true,
                attempts: None,
                log_tail: None,
                exit_code: None,
            }));
        };
        let java = PathBuf::from(&found);
        // 快照里那枚已经不在这台机器上了（卸载、换盘符、换机）：退回自动那一枚继续跑，
        // 但要把换过说在日志里 —— 不然报告写着 Java 21、日志用的却是另一枚，查起来两头对不上
        if probe.selected_missing {
            push_log(
                &a,
                &s,
                &i,
                PipelineStage::Installer,
                LogLevel::Warn,
                "方案里指定的那枚 Java 已不在本机，改用自动挑到的那一枚",
            );
        }
        push_log(
            &a,
            &s,
            &i,
            PipelineStage::Installer,
            LogLevel::Info,
            &format!(
                "本机安装 {subject} · Java {} · {found}",
                probe.major.map(|m| m.to_string()).unwrap_or_else(|| "?".into())
            ),
        );

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
        };
        // 安装器侧每 500ms 才扫一次目录，节拍已经比取件慢一个量级，不再另攒窗口
        let mut on_event = |ev: InstallEvent| match ev {
            InstallEvent::Log(line) => {
                push_log(&a, &s, &i, PipelineStage::Installer, LogLevel::Info, line)
            }
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
                    &install_done_line(&v, &subject),
                );
                // 落点要说出来：复用模式下这里就是缓存桶的坐标，用户排查「装到哪去了」只靠这一行
                push_log(
                    &a,
                    &s,
                    &i,
                    PipelineStage::Installer,
                    LogLevel::Info,
                    &format!("安装目录 {}", v.dir.display()),
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
        Err(e) => Err(InstallStop::Failed(TaskError {
            stage: PipelineStage::Installer,
            title: "本机安装异常".into(),
            detail: format!("安装线程异常：{e}"),
            code: None,
            retryable: true,
            attempts: None,
            log_tail: None,
            exit_code: None,
        })),
    }
}

/// 安装成功的收场行：命中复用与真装出来的说法必须分开——前者一次进程都没起。
/// 顶层布局也写进来：第 6 步的启动脚本三分叉就是按「有没有 run 脚本 / 顶层是不是散 jar」判的，
/// 现在让它先在日志里可见，装错了能在这一行看出来
pub(super) fn install_done_line(installed: &Installed, subject: &str) -> String {
    let counts = format!(
        "{} 个文件 · {}",
        installed.report.files,
        fmt_size(installed.report.bytes)
    );
    let layout = describe_layout(&installed.report.scripts, &installed.report.jars);
    if installed.from_cache {
        format!("复用已装的 {subject} · {counts} · 未起进程 · {layout}")
    } else {
        format!(
            "本机安装完成 · {subject} · {counts} · 用时 {:.1} 秒 · {layout}",
            installed.report.elapsed.as_secs_f64()
        )
    }
}

pub(super) fn describe_layout(scripts: &[String], jars: &[String]) -> String {
    let head = if scripts.is_empty() { None } else { Some(scripts.join("、")) };
    let tail = if jars.is_empty() { None } else { Some(jars.join("、")) };
    match (head, tail) {
        (Some(s), None) => format!("顶层 {s}"),
        (Some(s), Some(j)) => format!("顶层 {s} + {j}"),
        (None, Some(j)) => format!("无 run 脚本，顶层散 jar {j}"),
        (None, None) => "顶层既无 run 脚本也无散 jar".to_string(),
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
    let mut detail = e.to_string();
    if matches!(e, installer::InstallError::Timeout { .. }) {
        detail.push_str("，安装器进程已终止，半成品已回收");
    }
    TaskError {
        stage: PipelineStage::Installer,
        title: title.into(),
        detail,
        code: None,
        retryable: true,
        attempts: None,
        log_tail: None,
        exit_code: None,
    }
}
