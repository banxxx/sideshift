//! Tauri IPC 命令层：与 src/lib/api.ts 的 20 个命令契约一一对应。
//! 参数默认按 camelCase 暴露给 JS（Tauri v2 约定），JS 侧无需改名。

use std::path::PathBuf;
use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};
use tauri_plugin_opener::OpenerExt;

use crate::core::detector;
use crate::core::downloader::Downloader;
use crate::core::env;
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
        .map_err(|e| e.to_string())
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
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_java_versions() -> Vec<VersionOption> {
    ["8", "16", "17", "21"]
        .iter()
        .map(|v| VersionOption {
            value: v.to_string(),
            label: format!("Java {v}"),
            recommended: Some(*v == "17"),
            group: None,
        })
        .collect()
}

/// 由 MC 版本推默认 Java（Mojang 官方要求线）
fn java_for_mc(mc: &str) -> &'static str {
    let minor: i32 = mc
        .split('.')
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(20);
    let patch: i32 = mc
        .split('.')
        .nth(2)
        .and_then(|s| s.trim_start_matches(|c: char| !c.is_ascii_digit()).parse().ok())
        .unwrap_or(0);
    if (minor, patch) >= (20, 5) {
        "21"
    } else if minor >= 18 {
        "17"
    } else if minor >= 17 {
        "16"
    } else {
        "8"
    }
}

#[tauri::command]
pub fn default_options(state: S<'_>, manifest: PackManifest) -> ConversionOptions {
    let loader_version = lock(&state)
        .parsed_by_name
        .get(&manifest.file_name)
        .and_then(|p| p.loader_version.clone())
        .unwrap_or_default();
    ConversionOptions {
        mc_version: manifest.mc_version.clone(),
        loader_version,
        java_version: java_for_mc(&manifest.mc_version).to_string(),
        memory_mb: 4096,
        generate_scripts: true,
        nogui: true,
        agree_eula: false,
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
#[tauri::command]
pub async fn classify_pack(app: AppHandle, state: S<'_>) -> Result<PlanClassification, String> {
    // 快照 inputs：guard 必须在这个块里结束，否则 MutexGuard 跨 await 让命令 future 不 Send
    let (parsed, file_name, strip, online, cache_dir, concurrency) = {
        let inner = lock(&state);
        match last_parsed_of(&inner) {
            Some(p) => (
                p,
                inner.last_file.clone().unwrap_or_default(),
                inner.settings.strip_client_only,
                inner.settings.auto_classify_online,
                PathBuf::from(&inner.settings.cache_dir),
                inner.settings.concurrency.max(1) as usize,
            ),
            None => {
                return Ok(PlanClassification {
                    plan: Vec::new(),
                    online_pending: false,
                })
            }
        }
    };

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
    {
        let mut inner = lock(&state);
        inner.env_evidence = ev.clone();
        inner.env_code = code;
        inner.env_evidence_file = Some(file_name.clone());
    }
    // 离线那次推送：还有在线层要跑就说明本轮没结束（done=false，前端继续转圈）
    let offline_final = !online || pending.is_empty();
    emit_classified(
        &app,
        &file_name,
        plan.clone(),
        offline_final,
        offline_final,
    );

    // 在线层：只查离线没答上的那些行
    if online && !pending.is_empty() {
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
            let complete = env::resolve_online(
                &dl,
                &mut index,
                &cache_dir,
                &targets,
                &pending,
                &mut ev,
            )
            .await;
            let (plan, file) = {
                let mut g = lock(&state);
                // 反查期间用户可能换了包：不是同一个包就不落库、不推事件
                if g.env_evidence_file.as_deref() != Some(file_name.as_str()) {
                    return;
                }
                g.env_evidence = ev.clone();
                let plan = last_parsed_of(&g).map(|p| {
                    detector::build_plan(&p, g.settings.strip_client_only, &ev, &g.env_code)
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
    let (cache_dir, online, concurrency) = {
        let inner = lock(&state);
        (
            PathBuf::from(&inner.settings.cache_dir),
            inner.settings.auto_classify_online,
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
        .map_err(|e| e.to_string())?;
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
    mod_id: String,
) -> Result<Vec<ModVersionEntry>, String> {
    let mc = last_parsed(&state)
        .map(|p| p.manifest.mc_version.clone())
        .unwrap_or_else(|| "1.20.1".into());
    downloader_of(&state)
        .list_mod_versions(&mod_id, &mc)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_mod_categories(state: S<'_>) -> Result<Vec<String>, String> {
    downloader_of(&state)
        .list_mod_categories()
        .await
        .map_err(|e| e.to_string())
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

#[tauri::command]
pub async fn delete_task(app: AppHandle, state: S<'_>, id: String) -> Result<(), String> {
    {
        let mut inner = lock(&state);
        // 运行中的行不允许直接删（先取消）
        if inner.current.as_deref() == Some(id.as_str()) {
            return Ok(());
        }
        inner.tasks.remove(&id);
        inner.reports.remove(&id);
        inner.plans.remove(&id);
        inner.cancel.remove(&id);
        task_engine::save_tasks(&app, &inner);
    }
    // 行都删了，暂存目录没有留下的理由。guard 必须先在上面的块里结束：
    // remove_task_staging 会再锁同一把非重入 Mutex，嵌套即自死锁；命令 async 化后
    // 落盘与递归删除也不再占主线程（同步命令跑在主线程，慢 IO 会让窗口卡住）。
    task_engine::remove_task_staging(&state, &id);
    Ok(())
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

#[tauri::command]
pub async fn check_update(state: S<'_>) -> Result<String, String> {
    let dl = downloader_of(&state);
    let v = dl
        .client
        .get("https://api.github.com/repos/banxxx/sideshift/releases/latest")
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json::<serde_json::Value>()
        .await
        .map_err(|e| e.to_string())?;
    let tag = v["tag_name"].as_str().unwrap_or(env!("CARGO_PKG_VERSION"));
    Ok(tag.trim_start_matches('v').to_string())
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
