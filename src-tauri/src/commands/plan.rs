use super::*;

/* ---------------- 转换方案 ---------------- */


/// 联网反查是否还在为当前包跑：与 env_evidence 同一套「只认 last_file」的口径
fn online_running_of(inner: &task_engine::Inner) -> bool {
    inner.env_online_file.is_some() && inner.env_online_file == inner.last_file
}

/// 把「只有编号」的那些 CF 行补成带名字/大小/sha1 的行，并就地换掉解析缓存里那一份。
///
/// 补取只在自动分类入口跑一次，但它换掉的是**解析缓存那一份 Arc**，下载预估读的就是这一份
/// （页面链路里 classify 恒在 estimate 之前），所以三样东西同时到位：
/// - 名字：不然一排 `301445-4581013` 当模组名给用户看
/// - sha1：这是取证层第 2 层（按构建哈希反查端声明）的入场券，CF 包本来一条都没有，
///   补到 sha1 等于把「jar 自证」那一档缺失换来的取证能力补回半档
/// - 大小：不然下载预估把这些行按 0 计，报出来的数字是假的
///
/// 换缓存必须落在同一把锁里（分类那边快照过这同一份），否则这一轮补到的名字进不了方案
///
/// 路径那枚锚点**不许跟着名字飘**（见 `cfpack` 模块头）：任务存档里的方案行按 `src_path` 回指，
/// 一飘就把用户的手动改判全冲掉
///
/// 返回 `(按编号声明的行数, 要不要 CurseForge Key)`。第二个量**不看补取结果**：
/// 名字可以早被索引答过（那之后一度零请求、一度不需要 Key），但取字节每一步都要
/// `/download-url`，那是 CF 的接口、没 Key 就是拒绝。只看包里有几行编号声明 ⇒
/// 「同一个包第二次打开」这种热索引状态不再能把缺 Key 藏住
///
/// `reprobe` = 用户点「重新自动分类」：把索引里探过的取链许可作废再问一遍（`ensure` 那侧有理由）
async fn cf_enrich(state: &S<'_>, reprobe: bool) -> (usize, bool) {
    let (parsed, file_name, cache_dir) = {
        let inner = lock(&state);
        let Some(p) = last_parsed_of(&inner) else {
            return (0, false);
        };
        if cfpack::cf_row_count(&p) == 0 {
            return (0, false);
        }
        (
            p,
            inner.last_file.clone().unwrap_or_default(),
            PathBuf::from(&inner.settings.cache_dir),
        )
    };
    let rows = cfpack::cf_row_count(&parsed);
    let dl = downloader_of(state);
    let needs_key = !dl.has_curseforge_key();
    let out = cfpack::ensure(&dl, &cache_dir, &parsed, reprobe).await;
    // 一轮下来行内容一点没变（索引本来就热、或没 Key 一条没补到、也没许可态/端标签可写）⇒
    // 什么都不动，尤其别把 env 取证结论清掉：那会让同一个包切页往返又重跑一整轮离线探测
    if !out.renamed && !out.links_changed && !out.env_changed {
        return (rows, needs_key);
    }
    {
        let mut inner = lock(&state);
        // 补取期间用户可能换了包：不是同一个包就不落这一份（换了包就该按新包重算）
        if inner.last_file.as_deref() == Some(file_name.as_str()) {
            // 证据表按行归属，名字一变旧结论就对不上号 ⇒ 一并作废，让下一轮按新行重取。
            // 只改写许可态的那一轮不作废：端判定与「这枚模组拿不拿得到字节」无关
            if out.renamed
                && inner.env_evidence_file.as_deref() == Some(file_name.as_str())
            {
                inner.env_evidence.clear();
                inner.env_evidence_file = None;
                inner.env_code.clear();
            }
            inner
                .parsed_by_name
                .insert(file_name.clone(), Arc::new(out.parsed));
        }
    }
    (rows, needs_key)
}



/// 自动分类：先跑离线层（包内 jar 自证 + 本地索引）并立即返回，在线层（Modrinth 反查）
/// 后台补完再用 `plan://classified` 事件推一次增量。用户手改永远在前端 overrides 里，
/// 后端只交「自动结论」，不碰人工选择。
///
/// `force=false` 且这一包的端证据还在缓存里（= 同一个包第二次问）时直接复用取证结果：
/// 离线探测整轮跳过（那是这页最贵的重复劳动），方案按当前设置现算。
/// `force=true`（「重新自动分类」按钮）永远真重探一遍。
/// 但**联网那一轮不叠**：这一包已经有一轮在跑就不再起第二轮，离线重探照常做，
/// 剩下的行由在跑那一轮收尾时一起推（`online_pending=true` 让前端继续转圈）。
#[tauri::command]
pub async fn classify_pack(
    app: AppHandle,
    state: S<'_>,
    force: bool,
) -> Result<PlanClassification, String> {
    // 编号行先补取，再谈分类：这一步换掉的正是下面要快照的那一份解析缓存，排在快照之后就会
    // 拿着一份旧的名字去建方案（第一屏一排编号），而且 reuse 那条短路会把补取整个跳过
    let (cf_rows, cf_needs_key) = cf_enrich(&state, force).await;
    // 快照 inputs：guard 必须在这个块里结束，否则 MutexGuard 跨 await 让命令 future 不 Send
    let (parsed, file_name, strip, online, mirror, mcmod, cache_dir, concurrency, cached) = {
        let inner = lock(&state);
        let empty = env::EvidenceMap::new();
        match last_parsed_of(&inner) {
            Some(p) => {
                // 同一包第二次进来（切页再回、别的入口重问）：端证据已归属本包 ⇒ 整轮离线探测
                // 没有新东西可挖——probe_jars 要逐个开包内 jar，裸 zip 还得为缺 sha1 的行整包算
                // 哈希，而答案就是缓存里那批。方案仍然现算，所以设置页改 strip_client_only 照样
                // 立刻生效：缓存的是取证结果，不是结论。
                // force = 用户点「重新自动分类」，那条路必须真重探（也是联网失败后的重试出口）。
                let reuse = !force
                    && !inner.env_evidence.is_empty()
                    && inner.env_evidence_file.is_some()
                    && inner.env_evidence_file == inner.last_file;
                let cached = reuse.then(|| {
                    (
                        detector::build_plan(
                            &p,
                            inner.settings.strip_client_only,
                            evidence_of(&inner, &empty),
                            code_of(&inner),
                        ),
                        // 在线层还在跑 ⇒ 结论仍会由 classified 事件再推一次，别提前收「分类中」
                        online_running_of(&inner),
                    )
                });
                (
                    p,
                    inner.last_file.clone().unwrap_or_default(),
                    inner.settings.strip_client_only,
                    inner.settings.auto_classify_online,
                    inner.settings.env_lookup_mirror,
                    inner.settings.env_lookup_mcmod,
                    PathBuf::from(&inner.settings.cache_dir),
                    inner.settings.concurrency.max(1) as usize,
                    cached,
                )
            }
            None => {
                return Ok(PlanClassification {
                    plan: Vec::new(),
                    online_pending: false,
                    cf_rows,
                    cf_needs_key,
                })
            }
        }
    };
    if let Some((plan, online_running)) = cached {
        return Ok(PlanClassification {
            plan,
            online_pending: online && online_running,
            cf_rows,
            cf_needs_key,
        });
    }

    // 离线层 1：包内 jar 自证（Fabric/Quilt 的 environment + entrypoints）与自报身份/哈希
    // （重 CPU → _blocking）。不再被 mrpack env 挡住：jar 是最高可信层，本地扫描零请求成本，
    // 而且只有查了才知道打包者的声明有没有说谎（冲突要在行上标出来）
    let src = parsed
        .manifest
        .source_path
        .clone()
        .unwrap_or_default();
    // mrpack 声明层整包有没有区分度：全表刷 required/required 的默认值包要照常联网反查
    let informative = detector::mrpack_env_informative(&parsed.mod_files);
    let index = env::EnvIndex::load(&cache_dir);
    let need_jar: Vec<env::ProbeReq> = parsed
        .mod_files
        .iter()
        .filter(|f| f.in_pack)
        .map(|f| env::ProbeReq {
            path: f.path.clone(),
            // index 没给哈希的行才需要整包算 sha1（裸 zip 大包的额外开销就省在这一步）
            want_sha1: f.sha1.is_none(),
            // 字节码扫描只补「上面几层都答不上」的行：mrpack 有区分度地声明过、
            // 或本地索引已按哈希存过结论的，没必要再逐 class 走一遍常量池
            want_code: !detector::env_declared(f, informative)
                && !f.sha1.as_deref().is_some_and(|h| index.has_sha1(h)),
        })
        .collect();
    let probes = if src.is_empty() || need_jar.is_empty() {
        Default::default()
    } else {
        tauri::async_runtime::spawn_blocking(move || {
            env::probe_jars(&PathBuf::from(src), &need_jar)
        })
        .await
        .unwrap_or_default()
    };
    let mut ev: env::EvidenceMap = probes
        .iter()
        .filter_map(|(path, p)| p.env.map(|e| (path.clone(), e)))
        .collect();
    // 结构事实单独一张表：不进证据阶梯，只在 detector 里当名称层的闸门
    let code: env::CodeMap = probes
        .iter()
        .filter(|(_, p)| p.code != env::CodeFacts::default())
        .map(|(path, p)| (path.clone(), p.code))
        .collect();
    // 反查目标：index 未给哈希的行（裸 zip、手动塞入的 jar）用扫描算出的 sha1 补上——
    // 中文改名包只剩哈希与包内 id 这两条路能对上平台
    let mut targets = env::targets_for(&parsed.mod_files, informative);
    env::apply_probes(&probes, &mut targets);
    // 离线层 2：上次联网查到的本地索引（有则免去在线请求）
    let pending = env::apply_index(&index, &targets, &mut ev);
    // CF 构建级端标签（cfpack 从 cf-files-index.json 贴回行上的那枚）：作者上传时勾的
    // 官方声明，是 CF 清单行在 Modrinth 之外唯一的构建级证据。播种排在 jar 自证与索引
    // 结论之后——`put` 只认等档或更好，真正更优的旧结论不会被这枚新标签压掉
    for f in &parsed.mod_files {
        if let Some((c, s)) = f.cf.as_ref().and_then(|r| r.env) {
            env::put(
                &mut ev,
                &f.path,
                env::Evidence {
                    client: Some(c),
                    server: Some(s),
                    source: EnvSource::CfFile,
                },
            );
        }
    }

    let plan = detector::build_plan(&parsed, strip, &ev, &code);
    // 离线那次推送：还有在线层要跑就说明本轮没结束（done=false，前端继续转圈）
    let offline_final = !online || pending.is_empty();
    // 这一包是否已经有一轮联网反查在飞。**必须在下面立标记之前读**——那个标记写的就是
    // 「本轮还要联网」，先写后读会永远读到「有人在跑」，于是第二轮起不来、剩下的行再没人查。
    let round_running = !offline_final && online_running_of(&lock(&state));
    {
        let mut inner = lock(&state);
        inner.env_evidence = ev.clone();
        inner.env_code = code;
        inner.env_evidence_file = Some(file_name.clone());
        // 本轮确实还要联网：立个标记，让之后的缓存命中路径知道「结论还没最终化」。
        // 已有轮在跑时不重立：标记归那一轮清，我们这一趟并不起新轮，清了会把它的收尾闸门拆掉
        if !offline_final && !round_running {
            inner.env_online_file = Some(file_name.clone());
        }
    }
    emit_classified(
        &app,
        &file_name,
        plan.clone(),
        offline_final,
        offline_final,
    );

    // 在线层：只查离线没答上的那些行
    if online && !pending.is_empty() {
        // 同一包的联网轮不叠第二个：上一轮还在跑就把这一轮的收尾交给它。
        // 叠轮不只是多几条提示——两三轮查的是同一批行，白付一遍请求和时间，
        // 而每一轮各推一次收尾，前端就把同一句结论报好几遍。
        if round_running {
            // 方案现算、标记现读：那一轮可能刚好在这一趟期间收尾并把结论并进了证据表——
            // 拿上面那份离线 plan 回去会把它的结果吐掉，硬报 online_pending=true 则让前端一直转圈
            let (fresh, still_running) = {
                let g = lock(&state);
                let p = last_parsed_of(&g).map(|parsed| {
                    detector::build_plan(
                        &parsed,
                        g.settings.strip_client_only,
                        &g.env_evidence,
                        &g.env_code,
                    )
                });
                (p, g.env_online_file.as_deref() == Some(file_name.as_str()))
            };
            return Ok(PlanClassification {
                plan: fresh.unwrap_or(plan),
                online_pending: still_running,
                cf_rows,
                cf_needs_key,
            });
        }
        let app = app.clone();
        let state = state.inner().clone();
        let targets = targets.clone();
        tauri::async_runtime::spawn(async move {
            let dl = Downloader::new(cache_dir.clone(), concurrency);
            let mut index = env::EnvIndex::load(&cache_dir);
            let mut ev = state
                .inner
                .lock()
                .map(|g| g.env_evidence.clone())
                .unwrap_or_default();
            // 本轮会改写哪些行：收尾时按这份清单增量并回，不整表覆盖
            let touched: Vec<String> = pending
                .iter()
                .map(|i| targets[*i].path.clone())
                .collect();
            // 联网轮的读数（诊断用）：一次真请求至少一两百毫秒，所以"待查 N 行 + 用时几毫秒"
            // 就是"这一轮一条请求都没发出去"的铁证。发请求那条链在 Rust 进程里（不是 webview），
            // 前端的 Network 面板永远看不到，只能从这里出。
            let started = std::time::Instant::now();
            // 整轮墙钟预算：单次请求已经各掐 10s（downloader::client::METADATA_TIMEOUT），
            // 这一档掐的是"一百多个各慢一点"累出来的总账。到点就掐——`resolve_online` 按批
            // 落盘、结论又是就地写进 `ev` 的，所以已拿到的那部分照常生效（`out` 借用留在原地），
            // 只是不再有第二次机会补剩下的行；complete=false 让前端说「可重新自动分类」。
            let complete = match tokio::time::timeout(
                env::ONLINE_BUDGET,
                env::resolve_online(
                    &dl,
                    &mut index,
                    &cache_dir,
                    &targets,
                    &pending,
                    &mut ev,
                    mirror,
                    mcmod,
                ),
            )
            .await
            {
                Ok(ok) => ok,
                Err(_) => false,
            };
            let answered = touched.iter().filter(|p| ev.contains_key(p.as_str())).count();
            println!(
                "[env] 源={} 待查 {} 行 → 有结论 {} 行 · 完整={} · 用时 {}ms",
                if mirror { "麦块" } else { "官方" },
                pending.len(),
                answered,
                complete,
                started.elapsed().as_millis()
            );
            let (plan, file) = {
                let mut g = lock(&state);
                // 本轮收尾：只摘自己的标记。必须在下面换包那道闸门**之前**清——换包时这一趟会
                // 直接 return，写在闸门后面就永远清不掉；而标记留着，下次回到这个包会被
                // `round_running` 当成「还有一轮在跑」，于是那一包再也不会起联网轮（静默零请求）。
                // 换包那一趟已把标记写成新包的名字，所以这条 `== file_name` 的判断仍不会替它清。
                if g.env_online_file.as_deref() == Some(file_name.as_str()) {
                    g.env_online_file = None;
                }
                // 反查期间用户可能换了包：不是同一个包就不落库、不推事件
                if g.env_evidence_file.as_deref() != Some(file_name.as_str()) {
                    return;
                }
                // 只并回本轮查到的那些行，不整表覆盖：反查期间用户可能又点了一次「重新自动分类」，
                // 那一趟刚写过一批新的离线证据（还带着新的字节码事实），整表替换会把它们抹掉
                for path in &touched {
                    if let Some(one) = ev.get(path) {
                        g.env_evidence.insert(path.clone(), *one);
                    }
                }
                let plan = last_parsed_of(&g).map(|p| {
                    detector::build_plan(
                        &p,
                        g.settings.strip_client_only,
                        &g.env_evidence,
                        &g.env_code,
                    )
                });
                (plan, file_name.clone())
            };
            if let Some(plan) = plan {
                emit_classified(&app, &file, plan, true, complete);
            }
        });
    }
    Ok(PlanClassification {
        plan,
        online_pending: !offline_final,
        cf_needs_key,
        cf_rows,
    })
}

/// 推自动分类结果。分类明细不写日志行——日志量级就是前端性能预算（十五轮定案）
fn emit_classified(
    app: &AppHandle,
    file_name: &str,
    plan: Vec<PlanMod>,
    done: bool,
    complete: bool,
) {
    let _ = app.emit(
        task_engine::EVENT_CLASSIFIED,
        PlanClassified {
            file_name: file_name.to_string(),
            plan,
            done,
            complete,
        },
    );
}

