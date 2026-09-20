//! Tauri IPC 命令层：与 src/lib/api.ts 的 20 个命令契约一一对应。
//! 参数默认按 camelCase 暴露给 JS（Tauri v2 约定），JS 侧无需改名。

use std::path::PathBuf;
use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};

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
                    pack_env_untrusted: false,
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
        &ev,
        &parsed,
        informative,
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
            let (plan, file, parsed) = {
                let mut g = lock(&state);
                // 反查期间用户可能换了包：不是同一个包就不落库、不推事件
                if g.env_evidence_file.as_deref() != Some(file_name.as_str()) {
                    return;
                }
                g.env_evidence = ev.clone();
                let parsed = last_parsed_of(&g);
                let plan = parsed.as_ref().map(|p| {
                    detector::build_plan(p, g.settings.strip_client_only, &ev, &g.env_code)
                });
                (plan, file_name.clone(), parsed)
            };
            if let (Some(plan), Some(parsed)) = (plan, parsed) {
                emit_classified(
                    &app,
                    &file,
                    plan,
                    &ev,
                    &parsed,
                    informative,
                    true,
                    complete,
                );
            }
        });
    }
    Ok(PlanClassification {
        plan,
        online_pending: !offline_final,
        pack_env_untrusted: !informative,
    })
}

/// 推自动分类结果。分类明细不写日志行——日志量级就是前端性能预算（十五轮定案）
fn emit_classified(
    app: &AppHandle,
    file_name: &str,
    plan: Vec<PlanMod>,
    ev: &env::EvidenceMap,
    parsed: &ParsedPack,
    informative: bool,
    done: bool,
    complete: bool,
) {
    // 无证据 = 既没可信的整合包声明、没有 jar/平台证据、也没落进名称兜底的行
    let unresolved = parsed
        .mod_files
        .iter()
        .filter(|f| {
            !(detector::env_declared(f, informative)
                || ev.contains_key(&f.path)
                || detector::name_heuristic_hit(&f.file_name))
        })
        .count() as u32;
    let _ = app.emit(
        task_engine::EVENT_CLASSIFIED,
        PlanClassified {
            file_name: file_name.to_string(),
            plan,
            unresolved,
            done,
            complete,
            pack_env_untrusted: !informative,
        },
    );
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
pub fn delete_task(app: AppHandle, state: S<'_>, id: String) {
    let mut inner = lock(&state);
    // 运行中的行不允许直接删（先取消）
    if inner.current.as_deref() == Some(id.as_str()) {
        return;
    }
    inner.tasks.remove(&id);
    inner.reports.remove(&id);
    inner.plans.remove(&id);
    inner.cancel.remove(&id);
    task_engine::save_tasks(&app, &inner);
    // 行都删了，暂存目录没有留下的理由
    task_engine::remove_task_staging(&state, &id);
}

#[tauri::command]
pub fn get_report(state: S<'_>, task_id: String) -> Option<ConversionReport> {
    lock(&state).reports.get(&task_id).cloned()
}

/* ---------------- 设置 / 元信息 ---------------- */

#[tauri::command]
pub fn get_settings(state: S<'_>) -> AppSettings {
    lock(&state).settings.clone()
}

#[tauri::command]
pub fn set_settings(app: AppHandle, state: S<'_>, settings: AppSettings) {
    task_engine::save_settings(&app, &settings);
    lock(&state).settings = settings;
}

#[tauri::command]
pub fn list_download_sources() -> Vec<VersionOption> {
    vec![
        VersionOption {
            value: "official".into(),
            label: "官方源 · Mojang + Forge".into(),
            recommended: Some(true),
            group: None,
        },
        VersionOption {
            value: "bmclapi".into(),
            label: "BMCLAPI · 国内镜像".into(),
            recommended: Some(false),
            group: None,
        },
        VersionOption {
            value: "github".into(),
            label: "GitHub Releases".into(),
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
