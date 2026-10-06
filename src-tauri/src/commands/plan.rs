use std::path::Path;

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
/// 返回按编号声明的行数（给前端展示「这是 CF 官方导出包」的口径用）
///
/// `reprobe` = 用户点「重新自动分类」：把索引里探过的取链许可作废再问一遍（`ensure` 那侧有理由）
async fn cf_enrich(state: &S<'_>, reprobe: bool) -> usize {
    let (parsed, file_name, cache_dir) = {
        let inner = lock(&state);
        let Some(p) = last_parsed_of(&inner) else {
            return 0;
        };
        if cfpack::cf_row_count(&p) == 0 {
            return 0;
        }
        (
            p,
            inner.last_file.clone().unwrap_or_default(),
            PathBuf::from(&inner.settings.cache_dir),
        )
    };
    let rows = cfpack::cf_row_count(&parsed);
    let dl = downloader_of(state);
    let out = cfpack::ensure(&dl, &cache_dir, &parsed, reprobe).await;
    // 一轮下来行内容一点没变（索引本来就热、也没许可态/端标签可写）⇒
    // 什么都不动，尤其别把 env 取证结论清掉：那会让同一个包切页往返又重跑一整轮离线探测
    if !out.renamed && !out.links_changed && !out.env_changed {
        return rows;
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
    rows
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
    let cf_rows = cf_enrich(&state, force).await;
    // 快照 inputs：guard 必须在这个块里结束，否则 MutexGuard 跨 await 让命令 future 不 Send
    let (parsed, file_name, strip, env_source, mcmod, cache_dir, concurrency, cached) = {
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
                            meta_of(&inner),
                            &doubt_of(&inner),
                        ),
                        // 在线层还在跑 ⇒ 结论仍会由 classified 事件再推一次，别提前收「分类中」
                        online_running_of(&inner),
                    )
                });
                (
                    p,
                    inner.last_file.clone().unwrap_or_default(),
                    inner.settings.strip_client_only,
                    inner.settings.env_lookup_source,
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
                })
            }
        }
    };
    if let Some((plan, online_running)) = cached {
        return Ok(PlanClassification {
            plan,
            online_pending: !env_source.is_off() && online_running,
            cf_rows,
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
    let mut code: env::CodeMap = probes
        .iter()
        .filter(|(_, p)| p.code != env::CodeFacts::default())
        .map(|(path, p)| (path.clone(), p.code))
        .collect();
    // jar 自报身份（mod_id + 硬依赖）：detector 依赖映射与保护回路的数据源（B 层）
    let meta: env::MetaMap = probes
        .iter()
        .map(|(path, p)| (path.clone(), env::JarMeta::from(p)))
        .collect();
    // 反查目标：index 未给哈希的行（裸 zip、手动塞入的 jar）用扫描算出的 sha1 补上——
    // 中文改名包只剩哈希与包内 id 这两条路能对上平台
    let mut targets = env::targets_for(&parsed.mod_files, informative);
    env::apply_probes(&probes, &mut targets);
    // 离线层 2：上次联网查到的本地索引（有则免去在线请求）
    let pending = env::apply_index(&index, &targets, &mut ev);
    // 待查行的两个来源，`apply_index` 当场就分开了（不再靠「`ev` 里有没有这一行」倒推——那种数法
    // 必须卡在下面 CF 端标签播种**之前**，晚一步就把「新贴上的官方声明」误报成「索引里的过期结论」）：
    // - 过期重排 = 盘上有结论、只是 `ts` 空或超过 90 天，为了刷新时间戳又问一遍（结论照常垫底）。
    //   本轮重问过、平台还是给不出新答案 ⇒ `resolve_online` 收尾给它盖个新 `ts`，下一轮不再重问
    // - 无结论 = 索引压根没这一行，平台答不上也就没得记，下次进来原样再问（那要另立负记录）
    // CF 构建级端标签（cfpack 从 cf-files-index.json 贴回行上的那枚）：作者上传时勾的
    // 官方声明，是 CF 清单行在 Modrinth 之外唯一的构建级证据。播种排在 jar 自证与索引
    // 结论之后——`put` 只认等档或更好，真正更优的旧结论不会被这枚新标签压掉
    for f in &parsed.mod_files {
        if let Some((c, s)) = f.cf.as_ref().and_then(|r| r.env) {
            env::put(
                &mut ev,
                &f.path,
                env::Evidence {
                    client: c,
                    server: s,
                    source: EnvSource::CfFile,
                },
            );
        }
    }

    // A 层复核的存疑名单（离线部分）：索引里已记过的百科反驳照常生效，
    // 本轮在线复核的新增部分由后台轮并回（env_doubt）
    let doubt: std::collections::HashSet<String> =
        env::doubted_paths(&index, &targets, &ev).into_iter().collect();

    let plan = detector::build_plan(&parsed, strip, &ev, &code, &meta, &doubt);
    // 剔除行补验：声明判剔但没扫过字节的行在这里补上，矛盾会被 detector 按住（见 helper）。
    // src 已被上面那个 spawn_blocking 闭包 move 走，从解析结果里现取（同一个值）
    let plan = ensure_strip_facts(
        &parsed,
        plan,
        strip,
        &mut ev,
        &mut code,
        &meta,
        &doubt,
        Path::new(parsed.manifest.source_path.as_deref().unwrap_or_default()),
    )
    .await;
    // 在线轮的起跑条件扩了一项：除了没答上的行，还有「待剔除复核」的行时照样要跑——
    // 全表命中索引的二次分类里，复核腿是唯一还有事可做的一层
    let recheck_pending =
        !env_source.is_off() && env::strip_recheck_pending(&index, &targets, &ev);
    let offline_final = env_source.is_off() || (pending.is_empty() && !recheck_pending);
    // 这一包是否已经有一轮联网反查在飞。**必须在下面立标记之前读**——那个标记写的就是
    // 「本轮还要联网」，先写后读会永远读到「有人在跑」，于是第二轮起不来、剩下的行再没人查。
    let round_running = !offline_final && online_running_of(&lock(&state));
    {
        let mut inner = lock(&state);
        inner.env_evidence = ev.clone();
        inner.env_code = code;
        inner.env_meta = meta;
        inner.env_doubt = doubt;
        inner.env_evidence_file = Some(file_name.clone());
        // 本轮确实还要联网：立个标记，让之后的缓存命中路径知道「结论还没最终化」。
        // 已有轮在跑时不重立：标记归那一轮清，我们这一趟并不起新轮，清了会把它的收尾闸门拆掉
        if !offline_final && !round_running {
            inner.env_online_file = Some(file_name.clone());
        }
    }
    // 离线那次推送：还有在线层要跑就说明本轮没结束（done=false，前端继续转圈）
    emit_classified(
        &app,
        &file_name,
        plan.clone(),
        offline_final,
        offline_final,
    );

    // 在线层：只查离线没答上的那些行
    if !env_source.is_off() && !pending.is_empty() {
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
                        &g.env_meta,
                        &g.env_doubt,
                    )
                });
                (p, g.env_online_file.as_deref() == Some(file_name.as_str()))
            };
            return Ok(PlanClassification {
                plan: fresh.unwrap_or(plan),
                online_pending: still_running,
                cf_rows,
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
                .rows
                .iter()
                .map(|i| targets[*i].path.clone())
                .collect();
            // 整轮墙钟预算：单次请求已经各掐 10s（downloader::client::METADATA_TIMEOUT），
            // 这一档掐的是"一百多个各慢一点"累出来的总账。到点就掐——`resolve_online` 按批
            // 落盘、结论又是就地写进 `ev` 的，所以已拿到的那部分照常生效（`out` 借用留在原地），
            // 只是不再有第二次机会补剩下的行；`unwrap_or_default` 到点给 false，
            // complete=false 让前端说「可重新自动分类」。
            let complete = tokio::time::timeout(
                env::ONLINE_BUDGET,
                env::resolve_online(
                    &dl,
                    &mut index,
                    &cache_dir,
                    &targets,
                    &pending,
                    &mut ev,
                    env_source,
                ),
            )
            .await
            .unwrap_or_default();
            // CF slug 搜索反查：Modrinth 全落空的行用 jar 内 modId 直查 CF（收录远大于
            // Modrinth，且 CF slug 与 modId 同形率高，实测 biomesize 一击命中）。
            // 命中的行带项目级端标签（EnvSource::CfFile），下一层百科只兜仍然无结论的
            let _ = tokio::time::timeout(
                env::CF_SEARCH_BUDGET,
                env::resolve_via_cf_search(&dl, &mut index, &cache_dir, &targets, &mut ev),
            )
            .await;
            // 百科补全腿**独立于平台墙钟**：它串行且慢（一行两发，96 行预算能跑 1-3 分钟），
            // 挤在平台腿的 60 秒里只会被掐腰——那正是「百科里有数据的模组大量停在需人工」
            // 的原因（2026-10 实测 FarmingTales 包：6/6 模组百科都有正确声明）。
            // 单独给它一段自己的时间；到点掐掉时已答的行照常落盘，下一轮（重开页/重新分类）
            // 靠索引命中接着跑剩下的，渐进覆盖。mcmod 关着时零请求
            if mcmod {
                let _ = tokio::time::timeout(
                    env::MCMOD_BUDGET,
                    env::resolve_via_mcmod(&dl, &mut index, &cache_dir, &targets, &mut ev),
                )
                .await;
            }
            // A 层剔除复核腿：对「项目级/以下证据判剔除」的行追加问 CF + 百科（GeckoLib
            // 那类作者填错声明的模组在这里被纠正）。CF 命中直接覆盖证据；百科反驳进存疑
            // 名单。改动的行都要并回共享表：CF 覆盖改了证据，存疑行要进 env_doubt
            let (recheck_touched, recheck_doubt) = tokio::time::timeout(
                env::RECHECK_BUDGET,
                env::recheck_strip_rows(&dl, &mut index, &cache_dir, &targets, &mut ev, mcmod),
            )
            .await
            .unwrap_or_default();
            // —— 锁一：收尾标记 + 合并 touched + 克隆工作表（补扫在锁外做）——
            let work = {
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
                for path in touched.iter().chain(recheck_touched.iter()) {
                    if let Some(one) = ev.get(path) {
                        g.env_evidence.insert(path.clone(), *one);
                    }
                }
                // 复核存疑并入名单（detector 重算时按住剔除改保留+待人工）
                g.env_doubt.extend(recheck_doubt.iter().cloned());
                let parsed = last_parsed_of(&g);
                let strip = g.settings.strip_client_only;
                let src = parsed
                    .as_ref()
                    .and_then(|p| p.manifest.source_path.clone())
                    .unwrap_or_default();
                parsed.map(|p| {
                    (
                        p,
                        strip,
                        src,
                        g.env_evidence.clone(),
                        g.env_code.clone(),
                        g.env_meta.clone(),
                        g.env_doubt.clone(),
                    )
                })
            };
            let Some((parsed, strip, src, mut ev_w, mut code_w, meta_w, doubt_w)) = work else {
                return;
            };
            // —— 锁外：剔除行补验（可能把矛盾剔除按回保留）；方案在锁二按合并后的表重算 ——
            let base_plan = detector::build_plan(&parsed, strip, &ev_w, &code_w, &meta_w, &doubt_w);
            let _plan_v = ensure_strip_facts(
                &parsed,
                base_plan,
                strip,
                &mut ev_w,
                &mut code_w,
                &meta_w,
                &doubt_w,
                Path::new(&src),
            )
            .await;
            // —— 锁二：补扫产出并回共享表、按共享表重算推事件 ——
            let plan = {
                let mut g = lock(&state);
                // 补扫期间换了包：不是同一个包就不落库、不推事件
                if g.env_evidence_file.as_deref() != Some(file_name.as_str()) {
                    return;
                }
                g.env_evidence = ev_w;
                g.env_code = code_w;
                last_parsed_of(&g).map(|p| {
                    detector::build_plan(
                        &p,
                        g.settings.strip_client_only,
                        &g.env_evidence,
                        &g.env_code,
                        &g.env_meta,
                        &g.env_doubt,
                    )
                })
            };
            let file = file_name.clone();
            if let Some(plan) = plan {
                emit_classified(&app, &file, plan, true, complete);
            }
        });
    }
    Ok(PlanClassification {
        plan,
        online_pending: !offline_final,
        cf_rows,
    })
}


/// 剔除行字节码补验：判进剔除分组的包内行，若还没有字节码事实就补扫一遍，返回重算后的方案。
///
/// 为什么要补：首轮扫描的 `want_code` 有意跳过「声明已答上」的行（省掉逐 class 走常量池的成本），
/// 但声明类证据——mrpack env、索引结论、联网反查——都可能错。实测（FarmingTales 包，2026-10）：
/// GeckoLib 是打包者塞进 overrides 的索引外文件，hash 反查落空，被 Modrinth 项目级
/// `server_side=optional` 判成客户端模组——服务端缺它直接起不来。detector 的矛盾否决
/// （`bytecode_vetoed_strip`）要有 `server_code` 事实在手才生效，这一步就是那笔账的补付：
/// 只扫 Remove 分组里没有事实的包内行，命中服务端标记即提前收工。
/// 补扫若解出 jar 自证（rank 0），同样并进证据表——`put` 会让它压过声明层
#[allow(clippy::too_many_arguments)]
async fn ensure_strip_facts(
    parsed: &ParsedPack,
    plan: Vec<PlanMod>,
    strip: bool,
    ev: &mut env::EvidenceMap,
    code: &mut env::CodeMap,
    meta: &env::MetaMap,
    doubt: &std::collections::HashSet<String>,
    src: &Path,
) -> Vec<PlanMod> {
    // 手动模式没有自动剔除，Remove 分组为空，自然无事可做
    if !strip || src.as_os_str().is_empty() {
        return plan;
    }
    let need: Vec<String> = plan
        .iter()
        .filter(|m| m.disposition == ModDisposition::Remove)
        .filter_map(|m| m.src_path.clone())
        .filter(|p| !code.contains_key(p))
        .collect();
    if need.is_empty() {
        return plan;
    }
    let src_buf = src.to_path_buf();
    let reqs: Vec<env::ProbeReq> = need
        .iter()
        .map(|p| env::ProbeReq {
            path: p.clone(),
            want_sha1: false,
            want_code: true,
        })
        .collect();
    let probes = tauri::async_runtime::spawn_blocking(move || env::probe_jars(&src_buf, &reqs))
        .await
        .unwrap_or_default();
    let mut changed = false;
    for (path, probe) in probes {
        if let Some(e) = probe.env {
            env::put(ev, &path, e);
            changed = true;
        }
        if probe.code != env::CodeFacts::default() && code.get(&path) != Some(&probe.code) {
            code.insert(path.clone(), probe.code);
            changed = true;
        }
    }
    if changed {
        detector::build_plan(parsed, strip, ev, code, meta, doubt)
    } else {
        plan
    }
}

/// 推自动分类结果。分类明细不写日志行——日志量级就是前端性能预算（十五轮定案）分类明细不写日志行——日志量级就是前端性能预算（十五轮定案）
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

#[cfg(test)]
mod strip_rescan_tests {
    use super::*;
    use crate::core::env::fixtures::{class_bytes, zip_bytes};
    use crate::core::parser::{PackFile, ParsedPack};
    use crate::models::PackManifest;

    /// 完整复刻 GeckoLib 场景（FarmingTales 包，2026-10 实测）：
    /// mrpack 声明有区分度、且把这一行判成剔除（server=unsupported）——
    /// 首轮扫描因此跳过它的字节码（want_code=false）。补扫解出 jar 里
    /// 确有服务端注册 ⇒ detector 按住剔除，改保留 + 待人工
    #[tokio::test]
    async fn strip_declared_row_gets_a_bytecode_second_chance() {
        // 源包：一个含服务端注册 class 的 Forge jar（mods.toml 无端字段）
        let jar = zip_bytes(&[
            (
                "META-INF/mods.toml",
                br#"modId = "geckolib"
displayName = "GeckoLib"
"#,
            ),
            (
                "com/example/Setup.class",
                class_bytes(&["com/example/Setup", "DeferredRegister", "net/minecraft/server/"]).as_slice(),
            ),
        ]);
        let dir = std::env::temp_dir().join(format!("ss-strip-rescan-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pack_path = dir.join("pack.zip");
        std::fs::write(&pack_path, zip_bytes(&[("overrides/mods/[前置]geckolib-forge-1.20.1-4.8.4.jar", &jar)])).unwrap();

        // mrpack 清单：区分度由另一行的 optional 提供（真实打包工具的整表刷法），
        // geckolib 行被判剔除（server=unsupported）
        let row = |name: &str, server: SideFlag, client: SideFlag| PackFile {
            path: format!("overrides/mods/{name}"),
            file_name: name.into(),
            url: String::new(),
            sha1: None,
            size_bytes: 10,
            in_pack: true,
            env_server: Some(server),
            env_client: Some(client),
            depends: Vec::new(),
            cf: None,
        };
        let parsed = ParsedPack {
            manifest: PackManifest {
                file_name: "p.mrpack".into(),
                loader: LoaderKind::Forge,
                mc_version: "1.20.1".into(),
                mod_count: 2,
                size_bytes: 0,
                parsed: true,
                error: None,
                source_path: Some(pack_path.display().to_string()),
            },
            mod_files: vec![
                row("[前置]geckolib-forge-1.20.1-4.8.4.jar", SideFlag::Unsupported, SideFlag::Required),
                row("some-client-thing.jar", SideFlag::Optional, SideFlag::Required),
            ],
            extra_files: Vec::new(),
            loader_version: None,
            root_prefix: String::new(),
        };

        let mut ev = env::EvidenceMap::new();
        let mut code = env::CodeMap::new();
        let plan = detector::build_plan(
            &parsed,
            true,
            &ev,
            &code,
            &Default::default(),
            &Default::default(),
        );
        assert_eq!(
            plan[0].disposition,
            ModDisposition::Remove,
            "补扫前：mrpack 声明把它判进剔除分组（声明可能是错的）"
        );

        // 补扫：解出服务端注册 ⇒ detector 的矛盾否决生效
        let plan = ensure_strip_facts(
            &parsed,
            plan,
            true,
            &mut ev,
            &mut code,
            &Default::default(),
            &Default::default(),
            Path::new(&pack_path),
        )
        .await;
        assert_eq!(
            plan[0].disposition,
            ModDisposition::Keep,
            "字节里确有服务端注册，剔除被按住"
        );
        assert!(plan[0].needs_review, "矛盾必须亮给人看");
        assert_eq!(plan[0].bytecode_hint, Some(BytecodeHint::ServerCode));

        let _ = std::fs::remove_dir_all(dir);
    }
}

#[cfg(test)]
mod e2e_diag {
    use super::*;
    use crate::core::downloader::Downloader;
    use std::path::PathBuf;

    /// 端到端诊断（真联网、真实包路径）：跑完整分类链路并打印每行的判定与来源。
    /// `cargo test --lib e2e_diag -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "真联网：需要真实 mrpack 路径"]
    async fn e2e_diag_farmingtales() {
        let path = r"E:\Tencent\QQNT Files\FarmingTales_ForgeⅡ v1.5.1.mrpack";
        if !std::path::Path::new(path).is_file() {
            println!("包不存在，跳过");
            return;
        }
        let parsed = parser::parse(std::path::Path::new(path));
        let cache_dir = PathBuf::from(r"E:\SideShift\cache");
        let dl = Downloader::new(cache_dir.clone(), 4).with_modrinth_mirror(true);

        // —— 离线段（与 classify_pack 同构，抽不掉因 state 耦合）——
        let src = parsed.manifest.source_path.clone().unwrap_or_default();
        let informative = detector::mrpack_env_informative(&parsed.mod_files);
        let mut index = env::EnvIndex::load(&cache_dir);
        let need_jar: Vec<env::ProbeReq> = parsed
            .mod_files
            .iter()
            .filter(|f| f.in_pack)
            .map(|f| env::ProbeReq {
                path: f.path.clone(),
                want_sha1: f.sha1.is_none(),
                want_code: true, // 诊断模式：全扫字节码
            })
            .collect();
        let probes = if src.is_empty() || need_jar.is_empty() {
            Default::default()
        } else {
            tauri::async_runtime::spawn_blocking(move || {
                env::probe_jars(&PathBuf::from(&src), &need_jar)
            })
            .await
            .unwrap_or_default()
        };
        let mut ev: env::EvidenceMap = probes
            .iter()
            .filter_map(|(path, p)| p.env.map(|e| (path.clone(), e)))
            .collect();
        let code: env::CodeMap = probes
            .iter()
            .filter(|(_, p)| p.code != env::CodeFacts::default())
            .map(|(path, p)| (path.clone(), p.code))
            .collect();
        let mut targets = env::targets_for(&parsed.mod_files, informative);
        env::apply_probes(&probes, &mut targets);
        let pending = env::apply_index(&index, &targets, &mut ev);
        let plan = detector::build_plan(
            &parsed,
            true,
            &ev,
            &code,
            &Default::default(),
            &Default::default(),
        );
        let remove_before = plan.iter().filter(|m| m.disposition == ModDisposition::Remove).count();
        let review_before = plan.iter().filter(|m| m.needs_review).count();
        println!("[离线] Remove={remove_before} 待人工={review_before} pending={}", pending.rows.len());

        // —— 在线段 ——
        let _ = env::resolve_online(
            &dl, &mut index, &cache_dir, &targets, &pending, &mut ev, EnvLookupSource::Official,
        )
        .await;
        let _ = tokio::time::timeout(
            env::CF_SEARCH_BUDGET,
            env::resolve_via_cf_search(&dl, &mut index, &cache_dir, &targets, &mut ev),
        )
        .await;
        let _ = tokio::time::timeout(
            env::MCMOD_BUDGET,
            env::resolve_via_mcmod(&dl, &mut index, &cache_dir, &targets, &mut ev),
        )
        .await;
        let (_recheck_touched, recheck_doubt) = tokio::time::timeout(
            env::RECHECK_BUDGET,
            env::recheck_strip_rows(&dl, &mut index, &cache_dir, &targets, &mut ev, true),
        )
        .await
        .unwrap_or_default();
        println!("[复核] 存疑={} 改写={}", recheck_doubt.len(), _recheck_touched.len());
        let doubt: std::collections::HashSet<String> = recheck_doubt.into_iter().collect();
        let plan = detector::build_plan(
            &parsed,
            true,
            &ev,
            &code,
            &Default::default(),
            &doubt,
        );

        // —— 诊断打印 ——
        let mut by_source: std::collections::BTreeMap<String, usize> = Default::default();
        let mut pending_names: Vec<String> = Vec::new();
        for m in &plan {
            let slot = by_source.entry(format!("{:?}", m.env_source)).or_insert(0);
            *slot += 1;
            if m.needs_review {
                let ev = ev.get(m.src_path.as_deref().unwrap_or(""));
                pending_names.push(format!(
                    "{} [{:?}/{:?} {:?}]",
                    m.id,
                    m.client_side,
                    m.server_side,
                    ev.map(|e| e.source)
                ));
            }
        }
        println!("[终态] 按来源分布: {by_source:?}");
        println!("[终态] Remove={} Keep={} 需人工={}", plan.iter().filter(|m| m.disposition == ModDisposition::Remove).count(), plan.iter().filter(|m| m.disposition == ModDisposition::Keep).count(), plan.iter().filter(|m| m.needs_review).count());
        println!("--- 仍需人工的行（前 30）---");
        for n in pending_names.iter().take(30) {
            println!("  {n}");
        }
    }
}


    /// 单行 mcmod 链路验证（真联网）：FTB Library 实测百科有词条 3184。
    /// 目的：确认「搜索 → 词条 → 名字闸 → 运行环境」整条小链在哪一环断
    #[tokio::test]
    #[ignore = "真联网"]
    async fn mcmod_leg_single_line_probe() {
        let dl = Downloader::new(std::env::temp_dir().join("ss-diag"), 1);
        let mut index = env::EnvIndex::default();
        let mut out = env::EvidenceMap::new();
        let targets = vec![env::Target {
            path: "overrides/mods/[FTB]ftb-library-forge-2001.2.12.jar".into(),
            sha1: None,
            cf_fingerprint: None,
            in_pack: true,
            project_id: None,
            slugs: vec!["ftblibrary".into()],
            title: Some("FTB Library".into()),
        }];
        // 缓存目录指到临时区：cwd 指仓库目录的话证据缓存（env-index.json）会落进源码树
        let cache_dir = std::env::temp_dir().join("ss-diag-mcmod");
        env::resolve_via_mcmod(&dl, &mut index, &cache_dir, &targets, &mut out).await;
        println!("mcmod 腿结果: {:?}", out.get(&targets[0].path));
        assert!(out.contains_key(&targets[0].path), "FTB Library 百科有词条且名字同形，应当答上");
    }

    /// A 层剔除复核腿验证（真联网）：GeckoLib 在 Modrinth 的项目级声明是作者填错的
    /// `(required, optional)`——按它判剔除；CF 的最新构建两侧齐勾。复核腿应当用
    /// CF 证据（CfFile 等级更高）覆盖项目级结论，裁决翻成保留
    #[tokio::test]
    #[ignore = "真联网"]
    async fn recheck_leg_corrects_geckolib() {
        let dl = Downloader::new(std::env::temp_dir().join("ss-diag"), 1);
        let mut index = env::EnvIndex::default();
        let targets = vec![env::Target {
            path: "overrides/mods/geckolib-forge-1.20.1-4.8.4.jar".into(),
            sha1: None,
            cf_fingerprint: None,
            in_pack: true,
            project_id: None,
            slugs: vec!["geckolib".into()],
            title: Some("GeckoLib".into()),
        }];
        // 复核腿的入场券：项目级证据 + 按它判剔除
        let mut out = env::EvidenceMap::new();
        env::put(
            &mut out,
            &targets[0].path,
            env::Evidence {
                client: Some(crate::models::SideFlag::Required),
                server: Some(crate::models::SideFlag::Optional),
                source: crate::models::EnvSource::ModrinthProject,
            },
        );
        let (touched, doubt) = env::recheck_strip_rows(&dl, &mut index, &std::env::temp_dir().join("ss-diag-recheck"), &targets, &mut out, true).await;
        println!("touched={touched:?} doubt={doubt:?}");
        println!("复核后证据: {:?}", out.get(&targets[0].path));
        let ev = out.get(&targets[0].path).expect("复核腿应答上 GeckoLib");
        assert_eq!(ev.source, crate::models::EnvSource::CfFile, "CF 复核应覆盖项目级结论");
        assert_eq!(ev.server, Some(crate::models::SideFlag::Required));
        assert!(touched.contains(&targets[0].path));
        assert!(doubt.is_empty(), "CF 已接住，不应走存疑旁路");
    }
