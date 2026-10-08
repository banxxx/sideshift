//! Tauri IPC 命令层：前端调用按域拆在 src/lib/api/ 下，注册清单见 lib.rs 的 generate_handler。
//! 参数默认按 camelCase 暴露给 JS（Tauri v2 约定），JS 侧无需改名。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};
use tauri_plugin_opener::OpenerExt;

use crate::core::ack::{self, AckList};
use crate::core::cfpack;
use crate::core::cleanup;
use crate::core::detector;
use crate::core::downloader::{Downloader, app_code};
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

/// 收 `&Arc<AppState>` 而不是 `&S<'_>`：启动那一趟自动检查手里没有 `State` 句柄（它跨 `await`），
/// 只有一个克隆出来的 `Arc`。两处敲的是同一份下载器配置，不许各拼一遍
fn downloader_of(state: &Arc<AppState>) -> Downloader {
    let s = lock(&state).settings.clone();
    Downloader::new(PathBuf::from(&s.cache_dir), s.concurrency as usize)
        .with_source(s.download_source.normalized())
        .with_modrinth_mirror(s.modrinth_mirror)
}

fn last_parsed_of(inner: &task_engine::Inner) -> Option<Arc<ParsedPack>> {
    inner
        .last_pack
        .as_ref()
        .and_then(|f| inner.parsed_by_pack.get(f))
        .cloned()
}

/// 端证据表是否属于当前包（换包后旧证据一律作废，避免同名条目张冠李戴）
fn evidence_of<'a>(
    inner: &'a task_engine::Inner,
    empty: &'a env::EvidenceMap,
) -> &'a env::EvidenceMap {
    if inner.env_evidence_pack.is_some() && inner.env_evidence_pack == inner.last_pack {
        &inner.env_evidence
    } else {
        empty
    }
}

/// 字节码结构事实只随 env_evidence 一起写入、一起作废，所以共用同一个包名闸门
fn code_of(inner: &task_engine::Inner) -> &env::CodeMap {
    if inner.env_evidence_pack.is_some() && inner.env_evidence_pack == inner.last_pack {
        &inner.env_code
    } else {
        // 作废态要的是空表：&HashMap::default() 生命周期不够，用静态空表兜住
        static EMPTY: std::sync::OnceLock<env::CodeMap> = std::sync::OnceLock::new();
        EMPTY.get_or_init(env::CodeMap::new)
    }
}

/// jar 自报身份（mod_id + 硬依赖）只随 env_evidence 一起写入、一起作废，同一个包名闸门
fn meta_of(inner: &task_engine::Inner) -> &env::MetaMap {
    if inner.env_evidence_pack.is_some() && inner.env_evidence_pack == inner.last_pack {
        &inner.env_meta
    } else {
        static EMPTY: std::sync::OnceLock<env::MetaMap> = std::sync::OnceLock::new();
        EMPTY.get_or_init(env::MetaMap::new)
    }
}

/// 复核存疑名单与 env_evidence 同生共死，同一个包名闸门
fn doubt_of(inner: &task_engine::Inner) -> std::collections::HashSet<String> {
    if inner.env_evidence_pack.is_some() && inner.env_evidence_pack == inner.last_pack {
        inner.env_doubt.clone()
    } else {
        Default::default()
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
            meta_of(&inner),
            &doubt_of(&inner),
        ),
        None => Vec::new(),
    }
}

/* ---------------- 域拆分 ---------------- */

mod about;
mod mods;
mod pack;
mod plan;
mod settings;
mod system;
mod task;
mod templates;
mod update;

pub use about::*;
pub use mods::*;
pub use pack::*;
pub use plan::*;
pub use settings::*;
pub use system::*;
pub use task::*;
pub use templates::*;
pub use update::*;

/// 启动后那一趟自动检查：它不是命令（前端不调它），由 `lib.rs` 的 setup 自己 spawn
pub(crate) use update::startup_check;
