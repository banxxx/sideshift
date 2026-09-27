//! Tauri IPC 命令层：前端调用按域拆在 src/lib/api/ 下，注册清单见 lib.rs 的 generate_handler。
//! 参数默认按 camelCase 暴露给 JS（Tauri v2 约定），JS 侧无需改名。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};
use tauri_plugin_opener::OpenerExt;

use crate::core::ack::{self, AckList};
use crate::core::cleanup;
use crate::core::detector;
use crate::core::downloader::{Downloader, net_code, reqwest_code};
use crate::core::env;
use crate::core::java;
use crate::core::parser;
use crate::core::parser::ParsedPack;
use crate::models::*;
use crate::task_engine::{self, AppState};

type S<'a> = State<'a, Arc<AppState>>;

fn lock(state: &AppState) -> std::sync::MutexGuard<'_, task_engine::Inner> {
    state.inner.lock().unwrap()
}

fn last_parsed(state: &S<'_>) -> Option<Arc<ParsedPack>> {
    let inner = lock(&state);
    last_parsed_of(&inner)
}

fn downloader_of(state: &S<'_>) -> Downloader {
    let s = lock(&state).settings.clone();
    Downloader::new(PathBuf::from(&s.cache_dir), s.concurrency as usize)
        .with_source(s.download_source.normalized())
        .with_curseforge_key(s.curseforge_api_key.clone())
}

/* ---------------- 解析 / 选项 ---------------- */

#[tauri::command]
pub fn parse_pack(state: S<'_>, path: String) -> PackManifest {
    let parsed = Arc::new(parser::parse(&PathBuf::from(&path)));
    let manifest = parsed.manifest.clone();
    {
        let mut inner = lock(&state);
        inner
            .parsed_by_name
            .insert(manifest.file_name.clone(), parsed);
        inner.last_file = Some(manifest.file_name.clone());
    }
    manifest
}

/// 把「最近一次解析的包」指向指定包，供任务回看/改方案时用。
///
/// `get_plan` / `classify_pack` / `list_pack_dirs` 全都只认内存里的 `last_file`，而解析缓存
/// **不落盘**（tasks.json 只存任务/方案/报告）。从任务列表进转换方案页时，那条任务可能早已
/// 不是本轮解析的包：重启后缓存是空的（页面全空），中途选过别的包则是错的包（张冠李戴）。
/// 命中缓存只挪指针；未命中按 `sourcePath` 重解析，口径与流水线阶段 1 一致。
///
/// 返回 false = 缓存没有且源文件已不在（被移动/删除，或旧版本存档没记路径）。
/// 调用方据此降级：方案本身有任务快照可读，只有「包内目录树」这类要重解析的明细拿不到。
#[tauri::command]
pub fn ensure_parsed(state: S<'_>, manifest: PackManifest) -> bool {
    {
        let mut inner = lock(&state);
        if inner.parsed_by_name.contains_key(&manifest.file_name) {
            inner.last_file = Some(manifest.file_name.clone());
            return true;
        }
    }

    let Some(src) = manifest.source_path.as_deref() else {
        return false;
    };
    let path = PathBuf::from(src);
    if !path.exists() {
        return false;
    }
    // 解析在锁外：几百个 jar 的包读到这里要是还握着全局锁，别的命令全跟着排队
    let parsed = Arc::new(parser::parse(&path));
    let mut inner = lock(&state);
    inner
        .parsed_by_name
        .insert(manifest.file_name.clone(), parsed);
    inner.last_file = Some(manifest.file_name);
    true
}

#[tauri::command]
pub async fn list_mc_versions(state: S<'_>) -> Result<Vec<VersionOption>, String> {
    downloader_of(&state)
        .list_mc_versions()
        .await
        .map_err(|e| e.ipc_msg())
}

#[tauri::command]
pub async fn list_loader_versions(
    state: S<'_>,
    mc_version: String,
) -> Result<Vec<VersionOption>, String> {
    let loader = last_parsed(&state)
        .map(|p| p.manifest.loader)
        .unwrap_or(LoaderKind::Fabric);
    downloader_of(&state)
        .list_loader_versions(&mc_version, loader)
        .await
        .map_err(|e| e.ipc_msg())
}

/// 本机 JDK 探测（Rust: `probe_java`）。开关打开时这条要在**点转换之前**就在转换页上看得见：
/// 跑不成即失败，可预见的失败不该排到几十秒下载后面才爆出来。
///
/// `requiredVersion` 传当前方案那档 `javaVersion`（"17"）；传 null 只报「本机有什么」，不判够不够。
/// `javaPath` 传用户在手选框里指定的那一枚（空/null = 自动）。回包里带 `installed` 全列表，
/// 转换页那颗下拉的候选就是它 —— 所以这一条既是事前检查，也是候选来源，两处必是同一份事实。
/// 每次调用都重跑一趟、不落缓存：用户装完 JDK 回到页面就该变绿，缓存一个会随环境漂移的判定
/// 正是端判定那条链上被修掉过的病（见 `core::java` 模块头）。
#[tauri::command]
pub async fn probe_java(
    required_version: Option<String>,
    java_path: Option<String>,
) -> Result<JavaProbe, String> {
    tauri::async_runtime::spawn_blocking(move || java::probe(&required_version, &java_path))
        .await
        .map_err(|e| e.to_string())
}

/// 某 MC 版本的 Java 需求线（Rust: `java_requirement`）。给转换页换版本时改写方案用：
/// 这张表只在 `core::java` 里有一份，前端复刻一份就会跟实跑的那把筛子走偏。
/// 为什么由前端改写而不是消费方现算：`options.java_version` 是**快照字段**，回看与重试读的都是
/// 当时那一档；在报告或实跑里现算会让旧任务被新表改写。
#[tauri::command]
pub fn java_requirement(mc_version: String) -> String {
    java::required_for_mc(&mc_version).to_string()
}

#[tauri::command]
pub fn default_options(state: S<'_>, manifest: PackManifest) -> ConversionOptions {
    let (loader_version, install_loader_locally) = {
        let inner = lock(&state);
        (
            inner
                .parsed_by_name
                .get(&manifest.file_name)
                .and_then(|p| p.loader_version.clone())
                .unwrap_or_default(),
            inner.settings.install_loader_locally,
        )
    };
    ConversionOptions {
        mc_version: manifest.mc_version.clone(),
        loader_version,
        java_version: java::required_for_mc(&manifest.mc_version).to_string(),
        memory_mb: 4096,
        generate_scripts: true,
        nogui: true,
        // 默认开：关着做出来的包首次一律拒启（`eula.txt` 恒生成，这一档只决定里面的值）
        agree_eula: true,
        // 全局值只在这里当**初值**用一次：用户在本包改过就存进自己那份，之后重试与回看都读快照，
        // 不再回头看全局（否则同一份方案隔几天重跑会做出不同的包）
        install_loader_locally,
        ..Default::default()
    }
}

/// 最近一次解析包内可保留的目录树（mods 之外；文件数递归统计，同层按名升序）
#[tauri::command]
pub fn list_pack_dirs(state: S<'_>) -> Vec<PackDirNode> {
    let Some(p) = last_parsed(&state) else {
        return Vec::new();
    };

    #[derive(Default)]
    struct Node {
        counts: u32,
        children: std::collections::BTreeMap<String, Node>,
    }
    fn build(name: &str, node: &Node) -> PackDirNode {
        PackDirNode {
            name: name.to_string(),
            file_count: node.counts,
            // BTreeMap 迭代天然按 key 升序
            children: node.children.iter().map(|(n, c)| build(n, c)).collect(),
        }
    }

    let mut root = Node::default();
    // mods 由「模组方案」卡管理；resourcepacks 是客户端资源，服务端不消费——都不进保留树；
    // overrides/ 壳前缀剥离后再入树（CF 格式内容映射到包根，与拷贝口径一致）
    const TREE_SKIP_TOP: &[&str] = &["mods", "resourcepacks"];
    for f in &p.extra_files {
        let rel = parser::logical_rel(&f.path.replace('\\', "/")).to_lowercase();
        let segs: Vec<&str> = rel.split('/').collect();
        // 末段是文件名；根文件（pack.png 等）无目录段，不入树
        if segs.len() < 2 || TREE_SKIP_TOP.contains(&segs[0]) {
            continue;
        }
        let mut cur = &mut root;
        for seg in &segs[..segs.len() - 1] {
            cur.counts += 1;
            cur = cur.children.entry((*seg).to_string()).or_default();
        }
    }
    root.children.iter().map(|(n, c)| build(n, c)).collect()
}

/* ---------------- 转换方案 ---------------- */

fn last_parsed_of(inner: &task_engine::Inner) -> Option<Arc<ParsedPack>> {
    inner
        .last_file
        .as_ref()
        .and_then(|f| inner.parsed_by_name.get(f))
        .cloned()
}

/// 端证据表是否属于当前包（换包后旧证据一律作废，避免同名条目张冠李戴）
fn evidence_of<'a>(
    inner: &'a task_engine::Inner,
    empty: &'a env::EvidenceMap,
) -> &'a env::EvidenceMap {
    if inner.env_evidence_file.is_some() && inner.env_evidence_file == inner.last_file {
        &inner.env_evidence
    } else {
        empty
    }
}

/// 字节码结构事实只随 env_evidence 一起写入、一起作废，所以共用同一个包名闸门
fn code_of(inner: &task_engine::Inner) -> &env::CodeMap {
    if inner.env_evidence_file.is_some() && inner.env_evidence_file == inner.last_file {
        &inner.env_code
    } else {
        // 作废态要的是空表：&HashMap::default() 生命周期不够，用静态空表兜住
        static EMPTY: std::sync::OnceLock<env::CodeMap> = std::sync::OnceLock::new();
        EMPTY.get_or_init(env::CodeMap::new)
    }
}

/// 联网反查是否还在为当前包跑：与 env_evidence 同一套「只认 last_file」的口径
fn online_running_of(inner: &task_engine::Inner) -> bool {
    inner.env_online_file.is_some() && inner.env_online_file == inner.last_file
}

/// 最近一次解析包的方案（用户勾改在前端本地模型中，start_conversion 回传最终版）
fn current_plan(state: &S<'_>) -> Vec<PlanMod> {
    let inner = lock(&state);
    let empty = env::EvidenceMap::new();
    match last_parsed_of(&inner) {
        Some(p) => detector::build_plan(
            &p,
            inner.settings.strip_client_only,
            evidence_of(&inner, &empty),
            code_of(&inner),
        ),
        None => Vec::new(),
    }
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
    // 快照 inputs：guard 必须在这个块里结束，否则 MutexGuard 跨 await 让命令 future 不 Send
    let (parsed, file_name, strip, online, mirror, cache_dir, concurrency, cached) = {
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
                    PathBuf::from(&inner.settings.cache_dir),
                    inner.settings.concurrency.max(1) as usize,
                    cached,
                )
            }
            None => {
                return Ok(PlanClassification {
                    plan: Vec::new(),
                    online_pending: false,
                })
            }
        }
    };
    if let Some((plan, online_running)) = cached {
        return Ok(PlanClassification {
            plan,
            online_pending: online && online_running,
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

/// 「从本地添加」的单个 jar 取证：阶梯与整包分类完全一致（jar 自证 → 本地索引 → 联网反查），
/// 所以同一份 jar 第二次添加、或它本来就在包里时都是零请求。探测走 spawn_blocking：
/// 读文件 + 可能解几千个 class，不能占住 async 运行时
#[tauri::command]
pub async fn inspect_added_mod(state: S<'_>, path: String) -> Result<AddedModSide, String> {
    let (cache_dir, online, mirror, concurrency) = {
        let inner = lock(&state);
        (
            PathBuf::from(&inner.settings.cache_dir),
            inner.settings.auto_classify_online,
            inner.settings.env_lookup_mirror,
            inner.settings.concurrency.max(1) as usize,
        )
    };
    let p = PathBuf::from(&path);
    let file_name = p
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_string();
    let size_bytes = std::fs::metadata(&p).ok().map(|m| m.len());
    let probe = tauri::async_runtime::spawn_blocking({
        let p = p.clone();
        move || env::probe_local_jar(&p)
    })
    .await
    .unwrap_or_default();
    let ev = env::resolve_local_jar(
        &Downloader::new(cache_dir.clone(), concurrency),
        &cache_dir,
        &file_name,
        &probe,
        online,
        mirror,
    )
    .await;
    let (client_side, server_side, env_source) = match ev {
        Some(e) => (e.client, e.server, e.source),
        None => (None, None, EnvSource::Unknown),
    };
    Ok(AddedModSide {
        client_side,
        server_side,
        env_source,
        // 提示口径同 detector：服务端确有注册优先，否则才看纯客户端形状
        bytecode_hint: if probe.code.server_code {
            Some(BytecodeHint::ServerCode)
        } else if probe.code.client_only_shape {
            Some(BytecodeHint::ClientOnlyShape)
        } else {
            None
        },
        size_bytes,
        mod_id: probe.mod_id,
        title: probe.title,
    })
}

/// 「从网络添加」的单个构建补端。CurseForge 的响应体里没有任何端声明（Modrinth 有，
/// 所以只有 CF 那一档会走到这里），但它给了构建字节的 sha1 —— 同一份 jar 在两个平台哈希相同，
/// 于是拿这份哈希走与本地 jar 完全一致的阶梯：本地索引 → Modrinth 按哈希反查 → 项目/显示名。
/// 只读索引和发请求，不碰磁盘上的 jar，所以不需要 `spawn_blocking`。
/// 三层都没答上（含「联网反查」关着且索引没答过）→ `Unknown`，前端保持「无依据」，绝不猜
#[tauri::command]
pub async fn inspect_added_build(
    state: S<'_>,
    sha1: String,
    file_name: String,
    title: Option<String>,
) -> Result<AddedModSide, String> {
    let (cache_dir, online, mirror, concurrency) = {
        let inner = lock(&state);
        (
            PathBuf::from(&inner.settings.cache_dir),
            inner.settings.auto_classify_online,
            inner.settings.env_lookup_mirror,
            inner.settings.concurrency.max(1) as usize,
        )
    };
    let ev = env::resolve_added_build(
        &Downloader::new(cache_dir.clone(), concurrency),
        &cache_dir,
        &file_name,
        Some(&sha1),
        title.as_deref(),
        online,
        mirror,
    )
    .await;
    Ok(match ev {
        Some(e) => AddedModSide {
            client_side: e.client,
            server_side: e.server,
            env_source: e.source,
            ..Default::default()
        },
        // EnvSource 的 derive default 是 Mrpack（阶梯里的一个真档位），不是「无依据」
        None => AddedModSide {
            env_source: EnvSource::Unknown,
            ..Default::default()
        },
    })
}

#[tauri::command]
pub fn get_plan(state: S<'_>) -> Vec<PlanMod> {
    current_plan(&state)
}

#[tauri::command]
pub fn list_excluded_mods(state: S<'_>) -> Vec<PlanMod> {
    current_plan(&state)
        .into_iter()
        .filter(|m| m.disposition == ModDisposition::Remove)
        .collect()
}

/// 下载量预估：与构建取件同源分类 + 缓存扣减（core::estimate）；前端随方案/选项变化防抖调用
#[tauri::command]
pub async fn estimate_download(
    state: S<'_>,
    plan: Vec<PlanMod>,
    options: ConversionOptions,
) -> Result<DownloadEstimate, String> {
    let Some(parsed) = last_parsed(&state) else {
        return Ok(DownloadEstimate {
            download_bytes: 0,
            from_pack_bytes: 0,
            complete: false,
        });
    };
    let dl = downloader_of(&state);
    Ok(crate::core::estimate::estimate(&parsed, &plan, &options, &dl).await)
}

#[tauri::command]
pub async fn search_mods(
    state: S<'_>,
    query: ModSearchQuery,
) -> Result<ModSearchPage, String> {
    let page = downloader_of(&state)
        .search_mods(&query)
        .await
        .map_err(|e| e.ipc_msg())?;
    let added: Vec<String> = current_plan(&state)
        .into_iter()
        .filter(|m| m.disposition == ModDisposition::Add)
        .map(|m| m.id)
        .collect();
    Ok(ModSearchPage {
        results: page
            .results
            .into_iter()
            .map(|mut r| {
                r.already_added = added.contains(&r.id);
                r
            })
            .collect(),
        ..page
    })
}

#[tauri::command]
pub async fn list_mod_versions(
    state: S<'_>,
    source: ModSource,
    mod_id: String,
) -> Result<Vec<ModVersionEntry>, String> {
    let mc = last_parsed(&state)
        .map(|p| p.manifest.mc_version.clone())
        .unwrap_or_else(|| "1.20.1".into());
    downloader_of(&state)
        .list_mod_versions(source, &mod_id, &mc)
        .await
        .map_err(|e| e.ipc_msg())
}

#[tauri::command]
pub async fn list_mod_categories(
    state: S<'_>,
    source: ModSource,
) -> Result<Vec<String>, String> {
    downloader_of(&state)
        .list_mod_categories(source)
        .await
        .map_err(|e| e.ipc_msg())
}

/// 详情页那枚「翻译」按钮要的中文译文（麦块镜像 `detail/{slug}` 的 `title_zh` + `description_zh`，
/// 机器翻译件）。只在用户点击时发这一发，不进端判定的阶梯与预算，也不看「端信息反查源」那档设置——
/// 那档管的是自动分类查谁，这里查的是另一件事，关掉它不该让界面翻不了。
/// `Ok(None)` = 镜像名与简介都还没译文（不在收录、或长尾空串）⇒ 前端不切态、原样留着；
/// `Err` 只有网络故障一种，前端按「这次没翻成」提示，同样不动原文
#[tauri::command]
pub async fn mod_translate_zh(
    state: S<'_>,
    source: ModSource,
    slug: String,
) -> Result<Option<ModTranslation>, String> {
    downloader_of(&state)
        .translate_zh(source, &slug)
        .await
        .map_err(|e| e.ipc_msg())
}

/* ---------------- 任务生命周期 ---------------- */

#[tauri::command]
pub fn start_conversion(
    app: AppHandle,
    state: S<'_>,
    options: ConversionOptions,
    manifest: PackManifest,
    plan: Vec<PlanMod>,
) -> task_engine::StartResult {
    task_engine::create_task(&app, &state, options, manifest, plan)
}

/// 列表接口每条任务只带末尾这些行：进度事件到达时前端会全量重拉列表（Home 轨道取末 3 行、
/// 任务行取末 1 行），而单任务日志上限是 600 行——多条任务全量搬运就是 MB 级载荷。
/// 详情页与报告页走 getTask，仍是全量。
const LIST_LOG_TAIL: usize = 8;

#[tauri::command]
pub fn list_tasks(state: S<'_>) -> Vec<ConversionTask> {
    let mut v: Vec<ConversionTask> = lock(&state)
        .tasks
        .values()
        .cloned()
        .map(|mut t| {
            if t.logs.len() > LIST_LOG_TAIL {
                let cut = t.logs.len() - LIST_LOG_TAIL;
                t.logs.drain(..cut);
            }
            t
        })
        .collect();
    v.sort_by_key(|t| std::cmp::Reverse(t.created_at));
    v
}

#[tauri::command]
pub fn get_task(state: S<'_>, id: String) -> Option<ConversionTask> {
    lock(&state).tasks.get(&id).cloned()
}

#[tauri::command]
pub fn cancel_task(app: AppHandle, state: S<'_>, id: String) {
    task_engine::cancel(&app, &state, &id);
}

#[tauri::command]
pub fn retry_task(
    app: AppHandle,
    state: S<'_>,
    id: String,
) -> Option<task_engine::StartResult> {
    // 原地重试：同一 id、同一方案，产物落到自己上一份（详见 task_engine::retry_task）
    task_engine::retry_task(&app, &state, &id)
}

/// 删除 = 搬进回收站（撤回要用），所以**不**在这里递归删暂存目录——那笔账挪到了 clear_trash。
/// 记录本身已从 tasks.json 消失，所以进程退出后回收站自然空了（暂存残留由启动清扫兜底）。
#[tauri::command]
pub async fn delete_task(app: AppHandle, state: S<'_>, id: String) -> Result<(), String> {
    let mut inner = lock(&state);
    // 运行中的行不允许直接删（先取消）
    if inner.current.as_deref() == Some(id.as_str()) {
        return Ok(());
    }
    if task_engine::trash_task(&app, &mut inner, &id) {
        Ok(())
    } else {
        // 报出来而不是静默成就：走到这里说明前端把一条已经不在列表里的行又删了一次，
        // 那是状态机对不上（撤回/轮询竞态），不该被「反正结果一样」掩盖
        Err("这条任务已经不在列表里（可能刚被撤回或重复删除）".to_string())
    }
}

/// 回收站列表：只回弹窗要用的要点，不搬整份任务（日志能到几百行）
#[tauri::command]
pub fn list_trash(state: S<'_>) -> Vec<task_engine::TrashEntry> {
    task_engine::trash_entries(&lock(&state))
}

/// 撤回一条删除：任务连同方案与报告原样回到列表（暂存目录与产物一直没动）
#[tauri::command]
pub fn restore_task(app: AppHandle, state: S<'_>, id: String) -> Result<(), String> {
    let mut inner = lock(&state);
    task_engine::restore_task(&app, &mut inner, &id)
}

/// 清空回收站：这时才真正丢弃暂存目录。递归删除必须在解锁之后做——
/// remove_task_staging 会再锁同一把非重入 Mutex，嵌套即自死锁。
#[tauri::command]
pub async fn clear_trash(state: S<'_>) -> Result<usize, String> {
    let ids = {
        let mut inner = lock(&state);
        task_engine::drain_trash(&mut inner)
    };
    let n = ids.len();
    for id in ids {
        task_engine::remove_task_staging(&state, &id);
    }
    Ok(n)
}

#[tauri::command]
pub fn get_report(state: S<'_>, task_id: String) -> Option<ConversionReport> {
    lock(&state).reports.get(&task_id).cloned()
}

/// 某个任务创建时确认过的方案快照（报告页展开真实剔除/保留/新增清单）。
/// 不能用 `get_plan`：那个返回的是「最近一次解析的包」，用户换个包再看旧报告就会张冠李戴。
#[tauri::command]
pub fn get_task_plan(state: S<'_>, task_id: String) -> Vec<PlanMod> {
    lock(&state).plans.get(&task_id).cloned().unwrap_or_default()
}

/* ---------------- 设置 / 元信息 ---------------- */

#[tauri::command]
pub fn get_settings(state: S<'_>) -> AppSettings {
    lock(&state).settings.clone()
}

#[tauri::command]
pub fn set_settings(app: AppHandle, state: S<'_>, settings: AppSettings) -> Result<(), String> {
    // 手输/粘贴的目录可能带正斜杠，存下来一律先归成本机分隔符（否则 opener 打不开）
    let settings = settings.normalized();
    // 先落盘再改内存：写失败时内存仍是旧值，前端据此回滚，不会出现「界面已生效、重启又变回去」
    task_engine::save_settings(&app, &settings)?;
    lock(&state).settings = settings;
    Ok(())
}

/// 下载源档位。**只列真实存在的两条**：原来的「GitHub Releases」既不是 Maven 镜像、
/// 也没有任何代码走它，留着等于给用户一个假选项（镜像覆盖边界见 `core::downloader::source`）。
#[tauri::command]
pub fn list_download_sources() -> Vec<VersionOption> {
    vec![
        VersionOption {
            value: "official".into(),
            label: "官方源".into(),
            recommended: Some(true),
            group: None,
        },
        VersionOption {
            value: "bmclapi".into(),
            label: "BMCLAPI 国内镜像".into(),
            recommended: Some(false),
            group: None,
        },
    ]
}

/// 检查更新（Rust: check_update -> 该渠道的最新版本与是否有更新）。
///
/// 端点是 `/releases`（列表）而**不是** `/releases/latest`：官方定义 latest 只返回
/// "most recent non-prerelease, non-draft release"，所以 Beta 包在那条线上永远看不见自己该收的版本。
/// 渠道优先用用户在设置页选的；没选过时看这一枚包自己的版本号带不带预发布位——
/// 新装用户一个设置都没动也不会站错队。
/// 比较走 semver：`1.0.0-beta.2` 比 `1.0.0-beta.10` 新，字符串比较正好比反。
#[tauri::command]
pub async fn check_update(app: AppHandle, state: S<'_>) -> Result<UpdateInfo, String> {
    let picked = lock(&state).settings.update_channel;
    let current = app.package_info().version.to_string();
    let cur = semver::Version::parse(current.trim_start_matches('v'))
        .map_err(|e| format!("本地版本号不是合法 semver（{current}）：{e}"))?;
    let want_prerelease = picked.map(|c| c == UpdateChannel::Beta).unwrap_or(!cur.pre.is_empty());

    let dl = downloader_of(&state);
    const UPDATE_URL: &str = "https://api.github.com/repos/banxxx/sideshift/releases?per_page=30";
    let v = dl
        .client
        .get(UPDATE_URL)
        .send()
        .await
        .map_err(|e| reqwest_code(&e, UPDATE_URL))?
        .json::<serde_json::Value>()
        .await
        .map_err(|e| reqwest_code(&e, UPDATE_URL))?;
    // 仓库还没有任何 release 时 GitHub 回的是 `{"message": "Not Found"}` 对象，不是数组。
    // 失败这件事要原样递到界面上（不能装作"已是最新版本"），但它那句英文不用：按同一口径归类，
    // 前端出「GitHub 上没有找到对应内容」。限流单独归 `busy`——两者给用户的下一动作不一样
    let list = v.as_array().ok_or_else(|| {
        let msg = v["message"].as_str().unwrap_or("");
        net_code(UPDATE_URL, if msg.contains("rate limit") { 429 } else { 404 })
    })?;

    let mut best: Option<semver::Version> = None;
    for r in list {
        if r["draft"].as_bool().unwrap_or(false) {
            continue;
        }
        // 渠道对不上的一律跳过：正式版用户不该被推测试包，反之亦然
        if r["prerelease"].as_bool().unwrap_or(false) != want_prerelease {
            continue;
        }
        let Some(tag) = r["tag_name"].as_str() else { continue };
        let Ok(ver) = semver::Version::parse(tag.trim_start_matches('v')) else {
            continue;
        };
        if best.as_ref().map_or(true, |b| &ver > b) {
            best = Some(ver);
        }
    }

    Ok(UpdateInfo {
        has_update: best.as_ref().is_some_and(|b| b > &cur),
        latest: best.map(|b| b.to_string()),
        current,
    })
}

/* ---------------- 缓存占用与清理 ---------------- */

/// 缓存是否正在被用。运行中的流水线在往 `files` 写、往 `tasks` 暂存，排队的随时会被调度起来，
/// 两种都算忙——命令层据此挡掉「清空全部」。
fn cache_busy(inner: &task_engine::Inner) -> bool {
    inner
        .tasks
        .values()
        .any(|t| matches!(t.status, TaskStatus::Queued | TaskStatus::Running))
}

/// 从锁里只取出清理需要的三样事实（缓存目录 / 在册任务 id / 忙标志），扫描与删除全在锁外做：
/// 巨包的目录遍历是几秒级的 IO，握着全局锁不放就是「清理时整个应用点不动」。
fn cache_targets(
    inner: &task_engine::Inner,
) -> (PathBuf, std::collections::BTreeSet<String>, bool) {
    (
        PathBuf::from(&inner.settings.cache_dir),
        inner.tasks.keys().cloned().collect(),
        cache_busy(inner),
    )
}

#[tauri::command]
pub async fn cache_usage(state: S<'_>) -> Result<CacheUsage, String> {
    let (dir, ids, busy) = cache_targets(&lock(&state));
    tauri::async_runtime::spawn_blocking(move || cleanup::usage(&dir, &ids, busy))
        .await
        .map_err(|e| e.to_string())
}

/// 清理无用文件：半截下载 + 孤儿暂存目录 + 空壳目录。下载缓存本体一个字节都不碰，
/// 所以这条不挡运行中的任务（忙碌时扫描会自动跳过可能正在写的 `.part`）。
#[tauri::command]
pub async fn clean_junk(state: S<'_>) -> Result<CleanReport, String> {
    let (dir, ids, busy) = cache_targets(&lock(&state));
    tauri::async_runtime::spawn_blocking(move || cleanup::clean_junk(&dir, &ids, busy))
        .await
        .map_err(|e| e.to_string())
}

/// 清理下载缓存。`mode` = `stale`（只删过期）或 `all`（清空）。
///
/// 为什么只有 `all` 挡运行中的任务：`stale` 的判据是「最后一次使用」，而缓存每次命中复用都会
/// 刷新 mtime（`downloader::util::mark_used`），刚被在用的文件必然是 now，删不到它。
/// `all` 没有这层保护，正被流水线取用的文件说删就删。
#[tauri::command]
pub async fn clean_cache(state: S<'_>, mode: String) -> Result<CleanReport, String> {
    let (dir, _, busy) = cache_targets(&lock(&state));
    let mode = match mode.as_str() {
        "stale" => cleanup::CleanMode::Stale,
        "all" if !busy => cleanup::CleanMode::All,
        "all" => {
            return Err("有任务正在转换或排队中，清空缓存会删掉它在用的文件".into());
        }
        other => return Err(format!("未知的清理口径：{other}")),
    };
    tauri::async_runtime::spawn_blocking(move || cleanup::clean_cache(&dir, mode))
        .await
        .map_err(|e| e.to_string())
}

/* ---------------- 系统集成 ---------------- */

/// 用系统默认程序打开目录/文件（列表卡「打开输出目录」、报告页「打开文件夹」）。
///
/// 为什么在后端开而不让 JS 调 `openPath`：插件的 JS 命令受 capability scope 约束，
/// 只能命中清单里预先声明的目录（`$HOME/**` 那类），而输出目录是用户在原生对话框里
/// 自选的，可能是任意盘任意路径，枚举不完；命中不了就报 `opener:006 Not allowed to open path`。
/// 后端调用与其余文件 IO 同属可信代码，且分隔符在这里统一成本机写法，前端不必再关心。
#[tauri::command]
pub async fn open_local_path(app: AppHandle, path: String) -> Result<(), String> {
    app.opener()
        .open_path(native_path(&path), None::<&str>)
        .map_err(|e| e.to_string())
}

/// 在系统文件管理器中定位文件
#[tauri::command]
pub async fn reveal_local_path(app: AppHandle, path: String) -> Result<(), String> {
    app.opener()
        .reveal_item_in_dir(native_path(&path))
        .map_err(|e| e.to_string())
}

/* ---------------- 关于页：鸣谢名单 ---------------- */

/// 进关于页第一下读的那份：本机快照，零网络。没有快照就回 null，界面出「不可见 + 重新获取」。
#[tauri::command]
pub fn ack_snapshot(app: AppHandle) -> Option<AckList> {
    ack::read_snapshot(&app)
}

/// 拉一次远端名单（进页后台对账，以及空态上那枚「重新获取」）。
/// 失败只递 `net:` 码或 `ack:not-configured`，界面上两者同一个处理——这一块不抢全局提示区，
/// 也不把后端的英文句子摊在鸣谢名单里。
#[tauri::command]
pub async fn ack_refresh(app: AppHandle, state: S<'_>) -> Result<AckList, String> {
    let dl = downloader_of(&state);
    let list = ack::fetch_list(&dl.client).await?;
    ack::store(&app, &list);
    Ok(list)
}

/// 名单里 Minecraft 玩家的皮肤地址（`{显示名: https 贴图地址}`）。
///
/// **永远 `Ok`**，这是定的口径不是偷懒：这一条链上任何一档失败（没网、Mojang 限流、这个人已改名）
/// 在界面上都是同一件事——那张卡回落自带头像或名字首字，所以「失败」的形态是表里少一个人，
/// 不是一句关于页里没人该看见的错误。`Result` 这层壳是 Tauri 对带引用的异步命令的硬要求。
/// 缓存与 Mojang 的两道细节见 `core::ack` 的「正版皮肤」一节。
#[tauri::command]
pub async fn ack_skins(
    app: AppHandle,
    state: S<'_>,
    names: Vec<String>,
) -> Result<HashMap<String, String>, String> {
    let dl = downloader_of(&state);
    Ok(ack::fetch_skins(&app, &dl.client, &names).await)
}

/// 读本机存的一份皮肤贴图字节（base64）。`None`＝没有副本，前端自己去打 CDN。
/// 形状上的门槛（主机、内容哈希、大小）全在 `core::ack` 里，这里不加第二套
#[tauri::command]
pub fn ack_skin_texture_get(app: AppHandle, url: String) -> Option<String> {
    ack::read_texture(&app, &url)
}

/// 把刚取到的一份贴图字节存下来。**返回值只表示"这次落没落上"，调用方不必处理**：
/// 本次皮肤已经解出来了，缓存缺失的代价是下次再打一趟 CDN
#[tauri::command]
pub fn ack_skin_texture_put(app: AppHandle, url: String, data: String) -> bool {
    ack::write_texture(&app, &url, &data)
}
