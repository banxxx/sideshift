//! Tauri IPC 命令层：与 src/lib/api.ts 的 20 个命令契约一一对应。
//! 参数默认按 camelCase 暴露给 JS（Tauri v2 约定），JS 侧无需改名。

use std::path::PathBuf;
use std::sync::Arc;

use tauri::{AppHandle, State};

use crate::core::detector;
use crate::core::downloader::Downloader;
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
    inner
        .last_file
        .as_ref()
        .and_then(|f| inner.parsed_by_name.get(f))
        .cloned()
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

/// 最近一次解析包的方案（用户勾改在前端本地模型中，start_conversion 回传最终版）
fn current_plan(state: &S<'_>) -> Vec<PlanMod> {
    match last_parsed(state) {
        Some(p) => {
            let strip = lock(&state).settings.strip_client_only;
            detector::build_plan(&p, strip)
        }
        None => Vec::new(),
    }
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

/* ---------------- 任务生命周期 ---------------- */

#[tauri::command]
pub fn start_conversion(
    app: AppHandle,
    state: S<'_>,
    options: ConversionOptions,
    manifest: PackManifest,
    plan: Vec<PlanMod>,
) -> String {
    task_engine::create_task(&app, &state, options, manifest, plan)
}

#[tauri::command]
pub fn list_tasks(state: S<'_>) -> Vec<ConversionTask> {
    let mut v: Vec<ConversionTask> = lock(&state).tasks.values().cloned().collect();
    v.sort_by_key(|t| std::cmp::Reverse(t.created_at));
    v
}

#[tauri::command]
pub fn get_task(state: S<'_>, id: String) -> Option<ConversionTask> {
    lock(&state).tasks.get(&id).cloned()
}

#[tauri::command]
pub fn cancel_task(state: S<'_>, id: String) {
    task_engine::cancel(&state, &id);
}

#[tauri::command]
pub fn retry_task(
    app: AppHandle,
    state: S<'_>,
    id: String,
) -> Option<String> {
    let (options, pack, plan) = {
        let inner = lock(&state);
        let t = inner.tasks.get(&id)?;
        (
            t.options.clone(),
            t.pack.clone(),
            inner.plans.get(&id).cloned().unwrap_or_default(),
        )
    };
    Some(task_engine::create_task(&app, &state, options, pack, plan))
}

#[tauri::command]
pub fn delete_task(state: S<'_>, id: String) {
    let mut inner = lock(&state);
    inner.tasks.remove(&id);
    inner.reports.remove(&id);
    inner.plans.remove(&id);
    inner.cancel.remove(&id);
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
