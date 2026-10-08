//! 流水线：parser ≤15 · detector ≤30 ·（可选）本机装加载器 ≤42 · 取件 ≤82 · builder ≤100。
//! 阶段进度口径与前端 mock 引擎一致。本机安装排在模组取件**之前**（跑不成越早停越好），
//! 它从下载档头部切走 30→42 这一格；开关关着时主轮仍从 30 起，整条链路的写值逐字节相同。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use tauri::AppHandle;

use crate::core::builder::{self, BuildEvent, BuildInput, BuilderError};
use crate::core::cfpack;
use crate::core::detector;
use crate::core::downloader::{
    DownloadError, Downloader, Fetch, FetchSource, ItemSpec, TransferProgress,
};
use crate::core::installer::{self, InstallEvent, Installed};
use crate::core::java;
use crate::core::parser::{self, ParsedPack};
use crate::core::verify;
use crate::models::*;
use super::activity::{
    apply_fetch_counts, build_progress, fetch_group_label, flush_groups, group_line,
    group_plan_of, install_progress, reports_per_line, FetchGroups, NetActivity, ZipActivity,
    FETCH_FROM, INSTALL_FROM, INSTALL_TO, LOADER_JAR_TO,
};
use super::events::{
    emit_progress, fail, log_line, log_update, notify_done, push_line, push_log, update,
};
use super::persist::save_tasks;
use super::schedule::{release_and_next, remove_task_staging};
use super::state::{is_active, AppState};
use super::util::{brief_list, fmt_size, now_ms, sanitize, unique_mod_name};
use super::CACHE_TASKS_DIR;
use installer_stage::{
    prefetch_loader_jar, push_loader_item, run_installer_stage, InstallStage, InstallStop,
};
use readme::{build_readme, output_name_of};

mod installer_stage;
mod readme;

/// 在后台跑一条流水线；退出后回收暂存并拉起队首。
/// （原「独占槽位交接点」注释：流水线退出——成功/失败/取消皆如此——后交回调度器）
pub fn spawn_pipeline(app: &AppHandle, state: &Arc<AppState>, id: String) {
    let (a, s) = (app.clone(), state.clone());
    tauri::async_runtime::spawn(async move {
        run_pipeline(a.clone(), s.clone(), id.clone()).await;
        // 暂存目录只在**成功**后回收（产物已经打包，落位文件没用了）。
        // 失败与取消**保留**：重试沿用同一 id、同一方案，取件阶段对已落位的文件
        // 逐个校验直接复用（`download_all` 的复用闸），不用把几百 MB 重新来一遍。
        // 磁盘占用由删除任务（进回收站）与启动清扫兜底
        let succeeded = s
            .inner
            .lock()
            .map(|g| {
                g.tasks
                    .get(&id)
                    .is_some_and(|t| t.status == TaskStatus::Success)
            })
            .unwrap_or(false);
        if succeeded {
            remove_task_staging(&s, &id);
        }
        if let Some(next) = release_and_next(&a, &s, &id) {
            spawn_pipeline(&a, &s, next);
        }
    });
}

async fn run_pipeline(app: AppHandle, state: Arc<AppState>, id: String) {
    let (plan, pack, options) = {
        let inner = state.inner.lock().unwrap();
        let task = match inner.tasks.get(&id) {
            Some(t) => t,
            None => return,
        };
        // 排队期间被取消（或已删除）：不进入运行态，交回调度器拉起下一队
        if !matches!(task.status, TaskStatus::Queued | TaskStatus::Running) {
            return;
        }
        (
            inner.plans.get(&id).cloned().unwrap_or_default(),
            task.pack.clone(),
            task.options.clone(),
        )
    };

    /* ---- 阶段 1 · parser ---- */
    update(&app, &state, &id, |t| {
        t.status = TaskStatus::Running;
        t.stage = Some(PipelineStage::Parser);
        t.started_at = Some(now_ms());
        t.progress = 8;
    }, true);
    let parsed: Arc<ParsedPack> = {
        let cached = state.inner.lock().unwrap().parsed_by_pack.get(&pack.identity()).cloned();
        match cached {
            Some(p) => p,
            None => {
                let path = pack
                    .source_path
                    .as_deref()
                    .map(PathBuf::from)
                    .filter(|p| p.exists());
                match path {
                    Some(p) => Arc::new(parser::parse(&p)),
                    None => {
                        fail(&app, &state, &id, TaskError {
                            stage: PipelineStage::Parser,
                            title: "解析失败".into(),
                            detail: "找不到源整合包文件，请返回首页重新选择".into(),
                            code: None,
                            retryable: false,
                            attempts: None,
                            log_tail: None,
                            exit_code: None,
                        });
                        return;
                    }
                }
            }
        }
    };
    // CF 那一档（官方导出的包只给编号、jar 字节不在包里）：名字/大小/sha1 得向 CF 补。
    // 自动分类那一轮通常已补完并落进 `cf-files-index.json`，这里只把索引贴回行上；索引冷的档
    // （排队到重启后重跑、或分类那轮超时没查完）才真发请求——构建期每行反正还要现取一条直链，
    // 多这一发换来的是 sha1 能校验，值得
    let parsed: Arc<ParsedPack> = if cfpack::cf_row_count(&parsed) > 0 {
        let s = state.inner.lock().unwrap().settings.clone();
        let cache_dir = PathBuf::from(&s.cache_dir);
        let dl = Downloader::new(cache_dir.clone(), s.concurrency as usize)
            .with_modrinth_mirror(s.modrinth_mirror);
        let out = cfpack::ensure(&dl, &cache_dir, &parsed, false).await;
        if out.unresolved > 0 {
            push_log(
                &app,
                &state,
                &id,
                PipelineStage::Parser,
                LogLevel::Warn,
                &format!(
                    "CurseForge 编号补取：{} 行仍只有编号（本轮没查完），落位名按编号走",
                    out.unresolved
                ),
            );
        }
        Arc::new(out.parsed)
    } else {
        parsed
    };
    push_log(
        &app,
        &state,
        &id,
        PipelineStage::Parser,
        LogLevel::Info,
        &format!(
            "读取清单 {} · minecraft-{}",
            pack.file_name, parsed.manifest.mc_version
        ),
    );
    // 裸 zip 把内容整体套在一层自定义文件夹里很常见（`MyPack/mods/…`）。这一层不剥就会跟着
    // 勾选值与落位一起进产物，而服务端按实例根读 `config/`，等于整份保留内容放错了地方
    if !parsed.root_prefix.is_empty() {
        push_log(
            &app,
            &state,
            &id,
            PipelineStage::Parser,
            LogLevel::Info,
            &format!(
                "包根外层目录 {} 已按实例根处理（保留内容的勾选与落位都不带这一层）",
                parsed.root_prefix
            ),
        );
    }
    // index 声明了 URL 但包内没字节的残缺条目要联网补取；CF 那种「只有编号」的行同样没字节，
    // 只是它的 url 恒空（直链构建期现取），漏掉这一档就会把要下载的行报成「包内内容」
    let off_pack = parsed
        .mod_files
        .iter()
        .chain(parsed.extra_files.iter())
        .filter(|f| !f.in_pack && (!f.url.is_empty() || f.cf.is_some()))
        .count();
    push_log(
        &app,
        &state,
        &id,
        PipelineStage::Parser,
        LogLevel::Info,
        &format!(
            "包内内容：模组 {} 个 · 其他文件 {} 个 · 需联网补取 {} 个",
            parsed.mod_files.len(),
            parsed.extra_files.len(),
            off_pack
        ),
    );
    update(&app, &state, &id, |t| t.progress = 15, false);
    if !is_active(&state, &id) {
        return;
    }

    /* ---- 阶段 2 · detector ---- */
    update(&app, &state, &id, |t| t.stage = Some(PipelineStage::Detector), false);
    let counts = detector::count_plan(&plan);
    let review: Vec<String> = plan
        .iter()
        .filter(|m| m.needs_review)
        .map(|m| m.name.clone())
        .collect();
    push_log(
        &app,
        &state,
        &id,
        PipelineStage::Detector,
        LogLevel::Info,
        &format!("方案确认：剔除 {} · 保留 {} · 新增 {}", counts.remove, counts.keep, counts.add),
    );
    let removed: Vec<String> = plan
        .iter()
        .filter(|m| m.disposition == ModDisposition::Remove)
        .map(|m| m.name.clone())
        .collect();
    if !removed.is_empty() {
        push_log(
            &app,
            &state,
            &id,
            PipelineStage::Detector,
            LogLevel::Info,
            &format!("剔除名单：{}", brief_list(&removed)),
        );
    }
    for row in plan
        .iter()
        .filter(|m| m.auto_supplement && m.disposition != ModDisposition::Remove)
    {
        push_log(
            &app,
            &state,
            &id,
            PipelineStage::Detector,
            LogLevel::Info,
            &format!("自动补齐：{}（服务端运行所需）", row.name),
        );
    }
    if !review.is_empty() {
        push_log(
            &app,
            &state,
            &id,
            PipelineStage::Detector,
            LogLevel::Warn,
            &format!("待人工确认：{}（跨版本组件，服务端可能仍需）", brief_list(&review)),
        );
    }
    update(
        &app,
        &state,
        &id,
        |t| {
            t.counts = Some(counts);
            t.progress = 30;
        },
        true,
    );
    if !is_active(&state, &id) {
        return;
    }

    /* ---- 阶段 3 · downloader ---- */
    update(&app, &state, &id, |t| {
        t.stage = Some(PipelineStage::Downloader);
        t.progress = 31;
    }, true);
    let settings = state.inner.lock().unwrap().settings.clone();
    let staging = PathBuf::from(&settings.cache_dir)
        .join(CACHE_TASKS_DIR)
        .join(&id)
        .join("staging");
    let _ = std::fs::remove_dir_all(&staging);
    let mods_dir = staging.join("mods");
    let mut dl = Downloader::new(
        PathBuf::from(&settings.cache_dir),
        settings.concurrency as usize,
    )
    .with_source(settings.download_source.normalized())
    .with_modrinth_mirror(settings.modrinth_mirror);
    let source_path = parsed
        .manifest
        .source_path
        .clone()
        .map(PathBuf::from)
        .unwrap_or_default();

    // 3.0 缺件闸门：**问在下载之前**。自动分类那一轮逐枚探过取链许可（`cfpack::ensure`），
    // 「两条路都拿不到字节」的行标在方案行的 `cf_blocked` 上，所以这里一行网络请求都不发。
    // 少一枚 jar 的包在服上多半起不来，而那不是用户打算做的包 ⇒ 必选模组缺件要显式同意才放行；
    // 可选模组缺了不炸服，跳过并逐条写进报告就行。
    // 前端在「开始转换」那道上拦过一次，这一道是给旧任务存档与绕过界面的调用兜底：
    // 停在取件之前——几百枚 jar 下到一半才发现少一件，等于让用户白等一趟
    let blocked: Vec<&PlanMod> = plan
        .iter()
        .filter(|m| m.cf_blocked && m.disposition != ModDisposition::Remove)
        .collect();
    let blocked_names: Vec<String> = blocked.iter().map(|m| m.name.clone()).collect();
    let blocked_required = blocked.iter().filter(|m| m.cf_required).count();
    if blocked_required > 0 && !options.allow_missing_mods {
        fail(&app, &state, &id, TaskError {
            stage: PipelineStage::Downloader,
            title: "整合包里有模组拿不到文件".into(),
            detail: format!(
                "CurseForge 上这 {} 个模组不通过接口发放下载链，两条取链路都拿不到字节：{}。\
                 这是项目作者自己的设置，换什么网络都不会变。把它们在方案里改成剔除，\
                 或勾选「允许跳过拿不到文件的模组」再构建",
                blocked_required,
                brief_list(&blocked_names)
            ),
            code: None,
            retryable: false,
            attempts: None,
            log_tail: None,
            exit_code: None,
        });
        return;
    }
    // 放行到这一档：缺的那些不进产物，但必须留名（报告与 README 逐条列，不是静默丢）
    let skipped_mods: Vec<String> = blocked_names.clone();
    if !blocked_names.is_empty() {
        push_log(
            &app,
            &state,
            &id,
            PipelineStage::Downloader,
            LogLevel::Warn,
            &format!(
                "跳过 {} 个拿不到文件的模组（必选 {} 个，已同意）：{}",
                blocked_names.len(),
                blocked_required,
                brief_list(&blocked_names)
            ),
        );
    }

    let mut items: Vec<ItemSpec> = Vec::new();
    let mut used_files: HashSet<usize> = HashSet::new();
    // mods/ 落位文件名（大小写口径）：防不同目录同名 jar 静默互相覆盖
    let mut used_names: HashSet<String> = HashSet::new();
    let mut server_jar_name: Option<String> = None;
    let mut installer_jar_name: Option<String> = None;
    // 开了本机安装时，官方安装器 jar 被提前到「阶段 2.5」单独取，不进主轮计划：
    // 跑不成要越早停越好，不能让用户等完几 GB 模组才发现任务要重跑
    let mut loader_first: Option<ItemSpec> = None;

    // 3.1 保留 + 新增的模组
    for row in plan.iter().filter(|m| m.disposition != ModDisposition::Remove) {
        // 缺件闸门放行到这一档的那些：不进取件计划（拿不到字行的行只能整条不装），
        // 名字已经在上面向用户播报过，并逐条落在报告的「缺少模组」清单里
        if row.cf_blocked {
            continue;
        }
        // 在线添加的钉住行最先匹配：用户选哪个构建，构建时就下哪个（不再解析最新版）
        if let Some(p) = &row.pinned {
            let file_name = unique_mod_name(&mut used_names, &p.file_name, &row.id);
            // CurseForge 的直链是带时效的签名 URL，建档时存不下来 → 构建期现取一条。
            // 拿不到必须停下：空 URL 下载要么报错要么落一个废 jar 进服务端包
            let url = if p.needs_curseforge_link() {
                let file_id = p.file_id.clone().unwrap_or_default();
                match dl
                    .curseforge_build_url(&row.id, &file_id, &p.file_name, p.sha1.as_deref())
                    .await
                {
                    Ok(u) => u,
                    Err(e) => {
                        let detail = e.to_string();
                        fail(&app, &state, &id, TaskError {
                            stage: PipelineStage::Downloader,
                            title: "CurseForge 取链接失败".into(),
                            detail,
                            code: e.net_code(),
                            retryable: true,
                            attempts: None,
                            log_tail: None,
                            exit_code: None,
                        });
                        return;
                    }
                }
            } else {
                p.url.clone()
            };
            items.push(ItemSpec {
                fetch: Fetch::Url(url),
                sha1: p.sha1.clone(),
                dest: mods_dir.join(&file_name),
                file_name,
                size_bytes: row.size_bytes,
            });
            continue;
        }
        let matched = detector::match_pack_index(&parsed.mod_files, row, &used_files)
            .map(|i| (i, &parsed.mod_files[i]));
        if let Some((i, f)) = matched {
            used_files.insert(i);
            // CF 那一档：清单只给编号、字节不在包里，且**url 恒空**（直链带时效）。
            // 必须在「包内直取」那道判断**之前**分出去——`f.url.is_empty()` 那条写法会把这行
            // 当成 ZipEntry 去包里抽一个不存在的条目，报出来是「包内没有这个条目」，
            // 而真正的病因是没配 Key / CF 没答话。
            // 拿不到直链必须停下：空 URL 下载要么报错、要么落一个废 jar 进服务端包。
            // 被项目拒发 API 链那一档由 `curseforge_build_url` 内部退到推导链（只在有 sha1 锚时）
            let fetch = if let Some(cf) = &f.cf {
                match dl
                    .curseforge_build_url(&cf.mod_id, &cf.file_id, &f.file_name, f.sha1.as_deref())
                    .await
                {
                    Ok(u) => Fetch::Url(u),
                    Err(e) => {
                        let detail = e.to_string();
                        fail(&app, &state, &id, TaskError {
                            stage: PipelineStage::Downloader,
                            title: "CurseForge 取链接失败".into(),
                            detail,
                            code: e.net_code(),
                            retryable: true,
                            attempts: None,
                            log_tail: None,
                            exit_code: None,
                        });
                        return;
                    }
                }
            } else if f.in_pack || f.url.is_empty() {
                Fetch::ZipEntry {
                    archive: source_path.clone(),
                    entry: f.path.clone(),
                }
            } else {
                Fetch::Url(f.url.clone())
            };
            let file_name = unique_mod_name(&mut used_names, &f.file_name, &row.id);
            items.push(ItemSpec {
                fetch,
                sha1: f.sha1.clone(),
                dest: mods_dir.join(&file_name),
                file_name,
                size_bytes: f.size_bytes,
            });
        } else if let Some(lp) = &row.local_path {
            // 本地 .jar：直接取本地文件
            let lp = PathBuf::from(lp);
            let name = lp
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| format!("{}.jar", sanitize(&row.id)));
            let name = unique_mod_name(&mut used_names, &name, &row.id);
            let lp_size = std::fs::metadata(&lp).map(|m| m.len()).unwrap_or(0);
            items.push(ItemSpec {
                fetch: Fetch::Local(lp),
                file_name: name.clone(),
                sha1: None,
                dest: mods_dir.join(name),
                size_bytes: lp_size,
            });
        } else {
            // 外部新增：Modrinth 解析最新兼容构建
            match dl
                .resolve_mod_file(&row.id, &options.mc_version, parsed.manifest.loader)
                .await
            {
                Ok(mut spec) => {
                    spec.file_name = unique_mod_name(&mut used_names, &spec.file_name, &row.id);
                    spec.dest = mods_dir.join(&spec.file_name);
                    items.push(spec);
                }
                Err(e) => {
                    let detail = e.to_string();
                    fail(&app, &state, &id, TaskError {
                        stage: PipelineStage::Downloader,
                        title: "依赖解析失败".into(),
                        detail,
                        code: e.net_code(),
                        retryable: true,
                        attempts: None,
                        log_tail: None,
                        exit_code: None,
                    });
                    return;
                }
            }
        }
    }

    // 3.2 用户在「客户端保留内容」卡勾选的目录与文件（逻辑相对路径，任意层级），命中条目带入；
    // overrides/ 壳前缀剥离后匹配（CF 格式内容映射到服务端根）
    // 目录走 `{path}/` 前缀、文件走精确全等：两类勾选分成 keep_dirs / keep_files 两个字段，
    // 这里就不必拿字符串猜"这条到底是不是文件"（根级文件没有父目录，天然不与目录档冲突）
    // 落位是「勾哪一层就剪到哪一层」（parser::kept_rel）：勾选键有几段，交付路径就从第几段起算
    let mut kept_entries: HashMap<String, usize> = HashMap::new();
    for f in &parsed.extra_files {
        let rel = f.path.replace('\\', "/");
        let logical = parsed.logical_rel(&rel);
        let lower = logical.to_lowercase();
        // 保留范围硬闸：路径里出现 `mods` / `resourcepacks` 就不带（与保留树同一判据，见 parser::KEEP_SKIP_TOP）。
        // 前端勾不出这类条目，但它们可能来自旧草稿或手改的存档——只靠显示层挡等于给这里留后门
        if parser::keep_denied(&lower) {
            continue;
        }
        let hit = options
            .keep_dirs
            .iter()
            .find(|d| lower.starts_with(&format!("{}/", d.to_lowercase())))
            .or_else(|| options.keep_files.iter().find(|p| **p == lower))
            .cloned();
        let Some(entry) = hit else { continue };
        let landed = parser::kept_rel(&entry, logical);
        if landed.is_empty() {
            continue;
        }
        *kept_entries.entry(entry).or_default() += 1;
        let dest = staging.join(landed);
        let fetch = if f.in_pack || f.url.is_empty() {
            Fetch::ZipEntry {
                archive: source_path.clone(),
                entry: f.path.clone(),
            }
        } else {
            Fetch::Url(f.url.clone())
        };
        items.push(ItemSpec {
            fetch,
            file_name: f.file_name.clone(),
            sha1: f.sha1.clone(),
            dest,
            size_bytes: f.size_bytes,
        });
    }
    for dir in &options.keep_dirs {
        let n = kept_entries.get(dir).copied().unwrap_or(0);
        // 落位名与勾选名不同只在「勾了深层那档」时出现，日志把它写出来才读得懂产物里为什么没有上层
        let landed = parser::base_name(dir);
        let moved = if landed == dir {
            String::new()
        } else {
            format!(" → {landed}/")
        };
        push_log(
            &app,
            &state,
            &id,
            PipelineStage::Downloader,
            if n == 0 { LogLevel::Warn } else { LogLevel::Info },
            &format!(
                "保留目录 {dir}{moved} · {n} 个文件{}",
                if n == 0 { "（包内无此目录，已跳过）" } else { "" }
            ),
        );
    }
    for file in &options.keep_files {
        let n = kept_entries.get(file).copied().unwrap_or(0);
        let landed = parser::base_name(file);
        let moved = if landed == file {
            String::new()
        } else {
            format!(" → {landed}")
        };
        push_log(
            &app,
            &state,
            &id,
            PipelineStage::Downloader,
            if n == 0 { LogLevel::Warn } else { LogLevel::Info },
            &format!(
                "保留文件 {file}{moved}{}",
                if n == 0 { "（包内已无此文件，已跳过）" } else { "" }
            ),
        );
    }

    // 3.3 服务端加载器本体（loader_version 为空会拼出无效坐标——前端已拦截，这里兜底）
    if options.loader_version.trim().is_empty() {
        fail(&app, &state, &id, TaskError {
            stage: PipelineStage::Downloader,
            title: "未选择加载器版本".into(),
            detail: "裸 zip 包无法自动确定 Loader 版本，请在转换配置中选择后重试".into(),
            code: None,
            retryable: false,
            attempts: None,
            log_tail: None,
            exit_code: None,
        });
        return;
    }
    match parsed.manifest.loader {
        LoaderKind::Fabric => {
            match dl.fabric_server_jar(&options.mc_version, &options.loader_version).await {
                Ok(mut spec) => {
                    server_jar_name = Some(spec.file_name.clone());
                    spec.dest = staging.join(&spec.file_name);
                    items.push(spec);
                }
                Err(e) => {
                    fail(&app, &state, &id, TaskError {
                        stage: PipelineStage::Downloader,
                        title: "服务端加载器获取失败".into(),
                        detail: e.to_string(),
                        code: e.net_code(),
                        retryable: true,
                        attempts: None,
                        log_tail: None,
                        exit_code: None,
                    });
                    return;
                }
            }
        }
        LoaderKind::Forge => {
            let mut spec = dl.forge_installer(&options.mc_version, &options.loader_version);
            // maven 伴生 .sha1：补上后走统一校验与 sha1 键缓存
            dl.attach_side_sha1(&mut spec).await;
            installer_jar_name = Some(spec.file_name.clone());
            spec.dest = staging.join(&spec.file_name);
            push_loader_item(options.install_loader_locally, &mut items, &mut loader_first, spec);
        }
        LoaderKind::NeoForge => {
            let mut spec = dl.neoforge_installer(&options.loader_version);
            dl.attach_side_sha1(&mut spec).await;
            installer_jar_name = Some(spec.file_name.clone());
            spec.dest = staging.join(&spec.file_name);
            push_loader_item(options.install_loader_locally, &mut items, &mut loader_first, spec);
        }
    }

    let loader_jar = server_jar_name.clone().or(installer_jar_name.clone());
    if let Some(jar) = &loader_jar {
        push_log(
            &app,
            &state,
            &id,
            PipelineStage::Downloader,
            LogLevel::Info,
            &format!(
                "服务端加载器：{jar}（{} {}）",
                parsed.manifest.loader.as_label(),
                options.loader_version
            ),
        );
    }

    /* ---- 阶段 2.5 · 加载器就位（本机执行官方安装器，排在模组取件之前） ---- */
    // 排在这里的理由只有一条：本机装不出来是这条链路上最贵的失败（整条任务要重跑），
    // 放在几十 MB～几 GB 的模组下载之后，等于让用户等完才发现。
    // Fabric 走不到这里 —— 它没有安装器 jar 可提前（见 3.3 只在 Forge/NeoForge 分支落 loader_first）
    let cancel = state
        .inner
        .lock()
        .unwrap()
        .cancel
        .get(&id)
        .cloned()
        .unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
    // 装好的 loader 树：打包阶段并进 staging（并进去之后 staging 就等于产物内容）
    let mut installed: Option<Installed> = None;
    // 主轮取件的起脚：没走这一档仍是 30→82，写值与没有安装档时逐字节相同
    let fetch_from = match loader_first {
        Some(spec) => {
            let jar = spec.dest.clone();
            if let Err(e) = prefetch_loader_jar(&app, &state, &id, &mut dl, &cancel, spec).await {
                map_download_error(&app, &state, &id, &e);
                return;
            }
            if !is_active(&state, &id) {
                push_log(
                    &app,
                    &state,
                    &id,
                    PipelineStage::Downloader,
                    LogLevel::Warn,
                    "任务已取消，取件中止",
                );
                return;
            }
            // 复用关时装进任务私有目录：remove_task_staging 收的是 cache/tasks/{id} 整个目录，
            // 「用完即弃」不用再另起一条回收路径
            let scratch = staging
                .parent()
                .map(|p| p.join("install-scratch"))
                .unwrap_or_else(|| staging.join("install-scratch"));
            match run_installer_stage(
                &app,
                &state,
                &id,
                InstallStage {
                    loader: parsed.manifest.loader,
                    mc_version: &options.mc_version,
                    loader_version: &options.loader_version,
                    java_required: &options.java_version,
                    java_selected: &options.java_path,
                    reuse: settings.reuse_loader_installs,
                    cache_dir: Path::new(&settings.cache_dir),
                    installer_jar: &jar,
                    scratch: &scratch,
                    cancel: &cancel,
                },
            )
            .await
            {
                Ok(v) => {
                    installed = Some(v);
                    INSTALL_TO
                }
                Err(InstallStop::Cancelled) => {
                    push_log(
                        &app,
                        &state,
                        &id,
                        PipelineStage::Installer,
                        LogLevel::Warn,
                        "任务已取消，本机安装已中止",
                    );
                    return;
                }
                Err(InstallStop::Failed(e)) => {
                    fail(&app, &state, &id, e);
                    return;
                }
            }
        }
        None => FETCH_FROM,
    };

    let total = items.len() as u32;
    // 自检要对账「计划落进 mods/ 的文件」，items 随后被 download_all 吃掉，此刻快照一次
    let expected_mod_files: Vec<String> = items
        .iter()
        .filter(|i| i.dest.starts_with(&mods_dir))
        .map(|i| i.file_name.clone())
        .collect();
    // 取件构成：只有「Fetch::Url 且未命中下载缓存」的条目真正走网络，
    // 包内条目与本地 jar 零流量 —— 界面据此不再把全程称作假下载的「下载中」
    let mut tally = FetchTally {
        files: total,
        ..Default::default()
    };
    // 缓存探测要重算 sha1（整文件读一遍），条目多时不能反复读盘：一次算完给统计和分组计划共用
    let cached_flags: Vec<bool> = items.iter().map(|it| dl.is_cached(it)).collect();
    for (it, cached) in items.iter().zip(&cached_flags) {
        tally.bytes += it.size_bytes;
        // 与 harvest 的分拣一致：命中缓存的条目走复制，不再解包
        if *cached {
            tally.cached_files += 1;
        } else {
            match &it.fetch {
                Fetch::Url(_) => {
                    tally.net_files += 1;
                    tally.net_bytes += it.size_bytes;
                }
                Fetch::ZipEntry { .. } => tally.pack_files += 1,
                Fetch::Local(_) => tally.local_files += 1,
            }
        }
    }
    // 离线条目的分目录账本计划（入组口径与取件回调同源）
    let group_plan = group_plan_of(&items, &staging, &cached_flags);
    update(&app, &state, &id, |t| {
        t.total = Some(total);
        t.downloaded = Some(0);
        t.fetch = Some(tally);
        t.net_done = Some(0);
        t.done_bytes = Some(0);
    }, false);
    push_log(
        &app,
        &state,
        &id,
        PipelineStage::Downloader,
        LogLevel::Info,
        &format!(
            "取件计划 {total} 项 · 需联网 {}（≈{}）· 整合包 {} · 本地 {} · 缓存命中 {} · 并发 {}",
            tally.net_files,
            fmt_size(tally.net_bytes),
            tally.pack_files,
            tally.local_files,
            tally.cached_files,
            settings.concurrency
        ),
    );
    if tally.net_files == 0 {
        push_log(
            &app,
            &state,
            &id,
            PipelineStage::Downloader,
            LogLevel::Info,
            "本次无需联网：全部文件来自整合包、本地文件或下载缓存",
        );
    }

    let net_actual = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let bytes_actual = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let (net_a, bytes_a) = (net_actual.clone(), bytes_actual.clone());
    let groups = Arc::new(Mutex::new(FetchGroups::new(group_plan)));
    /* 联网实时条：字节级回调比日志密两个数量级，只攒账、按窗口出一条 activity；
       日志仍然一个条目完成一行 */
    let agg = Arc::new(Mutex::new(NetActivity::new(tally.net_bytes, tally.net_files)));
    let (app_t, state_t, id_t) = (app.clone(), state.clone(), id.clone());
    let (agg_t, net_t) = (agg.clone(), net_actual.clone());
    let cancel_t = cancel.clone();
    let dl = dl.with_transfer(Arc::new(move |p: &TransferProgress| {
        if cancel_t.load(Ordering::Relaxed) {
            return;
        }
        // 锁顺序固定「先账本、后任务表」，任务表回调里不得回头锁账本
        let info = agg_t
            .lock()
            .unwrap()
            .record(p, net_t.load(Ordering::Relaxed));
        if let Some(info) = info {
            update(&app_t, &state_t, &id_t, |t| t.activity = Some(info), true);
        }
    }));
    let (app_f, state_f, id_f) = (app.clone(), state.clone(), id.clone());
    let (groups_f, net_f, bytes_f) = (groups.clone(), net_actual.clone(), bytes_actual.clone());
    let (agg_f, net_a2) = (agg.clone(), net_actual.clone());
    let staging_f = staging.clone();
    // 取消后 release_and_next 会把下一条排队任务标成 Running，届时 is_active(本 id)
    // 这类「有没有 Running 行」的判断会误判为真，故回调自己盯取消标志
    let cancel_flag = cancel.clone();
    let dl_result = dl
        .download_all(items, cancel.clone(), move |done, tot, oc| {
            if cancel_flag.load(Ordering::Relaxed) {
                return;
            }
            let network = matches!(oc.source, FetchSource::Network);
            net_a.fetch_add(if network && !oc.cached { 1 } else { 0 }, Ordering::Relaxed);
            bytes_a.fetch_add(oc.bytes, Ordering::Relaxed);

            // 真正联网（含重试）的条目值得逐条盯：量级只有几十到几百条
            if reports_per_line(network, oc.cached, oc.retries) {
                let verb = if oc.cached { "复用缓存" } else { "联网获取" };
                let mut msg = format!("{verb} {} · {}", oc.file_name, fmt_size(oc.bytes));
                if oc.retries > 0 {
                    msg.push_str(&format!("（重试 {} 次后成功）", oc.retries));
                }
                let level = if oc.retries > 0 { LogLevel::Warn } else { LogLevel::Info };
                let (net_u, bytes_u) = (net_f.clone(), bytes_f.clone());
                // 结算这条的在飞字节；没有下一条在飞就顺势收掉实时条
                let snap = {
                    let mut g = agg_f.lock().unwrap();
                    g.settle(&oc.dest, oc.bytes);
                    (!g.inflight.is_empty()).then(|| g.snapshot(net_a2.load(Ordering::Relaxed)))
                };
                log_update(
                    &app_f,
                    &state_f,
                    &id_f,
                    PipelineStage::Downloader,
                    level,
                    &msg,
                    move |t| {
                        apply_fetch_counts(t, done, tot, &net_u, &bytes_u, fetch_from);
                        t.activity = snap;
                    },
                );
                return;
            }

            // 离线项：记进所属目录的账，取满预设数才出一条；没取满时只按间隔推进度、不写日志
            let label = fetch_group_label(&oc.dest, &staging_f);
            let done_snap = groups_f.lock().unwrap().record(&label, oc.bytes, done, tot);
            match done_snap {
                Some(g) => {
                    let (net_u, bytes_u) = (net_f.clone(), bytes_f.clone());
                    log_update(
                        &app_f,
                        &state_f,
                        &id_f,
                        PipelineStage::Downloader,
                        LogLevel::Info,
                        &group_line(&g),
                        move |t| apply_fetch_counts(t, g.done, g.total, &net_u, &bytes_u, fetch_from),
                    );
                }
                None => {
                    if groups_f.lock().unwrap().beat_due() {
                        let (net_u, bytes_u) = (net_f.clone(), bytes_f.clone());
                        update(
                            &app_f,
                            &state_f,
                            &id_f,
                            |t| apply_fetch_counts(t, done, tot, &net_u, &bytes_u, fetch_from),
                            true,
                        );
                    }
                }
            }
        })
        .await;
    // 兜底只给「跑完了但账本没对上」的正常任务补行；取消/失败不补，
    // 否则会把半截目录说成取件完成，还会覆写已取消任务的进度
    if dl_result.is_ok() && is_active(&state, &id) {
        flush_groups(&app, &state, &id, &groups, &net_actual, &bytes_actual, fetch_from);
    }
    if let Err(e) = dl_result {
        map_download_error(&app, &state, &id, &e);
        return;
    }
    if !is_active(&state, &id) {
        push_log(&app, &state, &id, PipelineStage::Downloader, LogLevel::Warn, "任务已取消，取件中止");
        return;
    }
    push_log(
        &app,
        &state,
        &id,
        PipelineStage::Downloader,
        LogLevel::Info,
        &format!(
            "取件完成 {total} 项 · 实际联网 {} 项 · 共取件 {}",
            net_actual.load(Ordering::Relaxed),
            fmt_size(bytes_actual.load(Ordering::Relaxed))
        ),
    );
    update(&app, &state, &id, |t| t.progress = 82, true);

    /* ---- 阶段 4 · builder ---- */
    // 联网实时条到此收摊，之后的进度由打包侧接管
    update(&app, &state, &id, |t| {
        t.stage = Some(PipelineStage::Builder);
        t.progress = 84;
        t.activity = None;
    }, true);
    let output_name = output_name_of(&pack.file_name);
    let readme = build_readme(
        &plan,
        &counts,
        &review,
        parsed.manifest.loader,
        &options.keep_dirs,
        &options.keep_files,
        options.agree_eula,
        installed.is_some(),
        &skipped_mods,
    );
    let build_state = state.clone();
    let build_app = app.clone();
    // 本次包的输出目录覆写：空则回落全局设置
    let output_dir = if options.output_override.trim().is_empty() {
        PathBuf::from(&settings.output_dir)
    } else {
        PathBuf::from(options.output_override.trim())
    };
    let build_options = options.clone();
    let build_loader = parsed.manifest.loader;
    let build_id = id.clone();
    let build_output_name = output_name.clone();
    let staging_path = staging.clone();
    // 本任务上一轮的产物：同名时覆写自己那份，不去抢别人占用的文件名
    let own_output = state
        .inner
        .lock()
        .unwrap()
        .tasks
        .get(&id)
        .and_then(|t| t.output_path.clone());
    let build_cancel = cancel.clone();
    // 自检开关：打包一结束、staging 还没回收时对账产物（离线六项，零子进程）
    let verify_on = settings.verify_after_build;
    let build_installed = installed.take();
    let build_result = tokio::task::spawn_blocking(move || {
        let mut agg = ZipActivity::default();
        let (a, s, i) = (build_app, build_state, build_id);
        let input = BuildInput {
            staging: &staging,
            output_dir: &output_dir,
            output_file_name: build_output_name,
            own_output,
            options: &build_options,
            loader: build_loader,
            server_jar_name,
            installer_jar_name,
            installed: build_installed.as_ref(),
            readme_lines: readme,
        };
        // 打包是 CPU + 磁盘活，进度事件按窗口节流；取消后不再写任务表
        let built = builder::build(&input, &mut |ev| {
            if build_cancel.load(Ordering::Relaxed) {
                return;
            }
            match ev {
                BuildEvent::Plan { files, bytes } => {
                    agg.plan(*files, *bytes);
                    let snap = agg.snapshot();
                    update(&a, &s, &i, |t| t.activity = Some(snap), true);
                }
                BuildEvent::File { group, bytes } => {
                    if let Some(snap) = agg.file(group, *bytes) {
                        let (done, total) = (snap.done_bytes, snap.total_bytes);
                        update(
                            &a,
                            &s,
                            &i,
                            move |t| {
                                t.activity = Some(snap);
                                t.progress = build_progress(done, total);
                            },
                            true,
                        );
                    }
                }
                BuildEvent::Group { label, files, bytes } => {
                    let snap = agg.snapshot();
                    let (done, total) = (snap.done_bytes, snap.total_bytes);
                    log_update(
                        &a,
                        &s,
                        &i,
                        PipelineStage::Builder,
                        LogLevel::Info,
                        &format!("已打包 · {label} · {files} 个 · {}", fmt_size(*bytes)),
                        move |t| {
                            t.activity = Some(snap);
                            t.progress = build_progress(done, total);
                        },
                    );
                }
            }
        })?;
        let checks = if verify_on {
            verify::run(&verify::Input {
                staging: &staging,
                options: &build_options,
                loader: build_loader,
                plan: &plan,
                // 认 builder 的结论而不是取件时那个 jar 名：已装的包里 installer jar 根本不进包，
                // 拿旧名字对账会把「装好了」报成「启动脚本指向的 jar 不在包里」
                start_jar: built.start_jar.as_deref(),
                args_files: &built.args_files,
                installed: build_installed.is_some(),
                generated: &built.generated,
                expected_mod_files: &expected_mod_files,
                expected_kept: &kept_entries,
            })
        } else {
            Vec::new()
        };
        Ok::<_, BuilderError>((built, checks))
    })
    .await;

    let (built, checks) = match build_result {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => {
            fail(&app, &state, &id, TaskError {
                stage: PipelineStage::Builder,
                title: "构建失败".into(),
                detail: e.to_string(),
                code: None,
                retryable: true,
                attempts: None,
                log_tail: Some(vec![e.to_string()]),
                exit_code: None,
            });
            return;
        }
        Err(e) => {
            fail(&app, &state, &id, TaskError {
                stage: PipelineStage::Builder,
                title: "构建失败".into(),
                detail: format!("构建线程异常：{e}"),
                code: None,
                retryable: true,
                attempts: None,
                log_tail: None,
                exit_code: None,
            });
            return;
        }
    };
    /* ---- 成功收尾 ---- */
    // 实际落盘名可能与默认名不同（撞名自动加了序号）：日志与报告都按实际名说
    let final_name = built
        .path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| output_name.clone());
    push_log(
        &app,
        &state,
        &id,
        PipelineStage::Builder,
        LogLevel::Info,
        &format!("生成包根文件：{}", brief_list(&built.generated)),
    );
    // 保留内容里勾了同名包根文件 ⇒ 以包内那份为准（用户主动要自己那份）。这几个配置项这次没进产物，
    // 必须在日志里点名，否则报告页播报的还是界面上填的那套数
    if !built.reused_root.is_empty() {
        push_log(
            &app,
            &state,
            &id,
            PipelineStage::Builder,
            LogLevel::Warn,
            &format!(
                "沿用包内自带的 {}：这些包根文件本次未按配置生成",
                brief_list(&built.reused_root)
            ),
        );
    }
    if built.overwritten {
        push_log(
            &app,
            &state,
            &id,
            PipelineStage::Builder,
            LogLevel::Info,
            &format!("覆写本任务上一次的输出：{final_name}"),
        );
    } else if final_name != output_name {
        push_log(
            &app,
            &state,
            &id,
            PipelineStage::Builder,
            LogLevel::Warn,
            &format!("{output_name} 已被其他包占用，本次另存为 {final_name}"),
        );
    }
    push_log(
        &app,
        &state,
        &id,
        PipelineStage::Builder,
        LogLevel::Info,
        &format!(
            "打包 {} · {} 个文件 · {}",
            final_name,
            built.entries,
            fmt_size(built.size)
        ),
    );
    push_log(
        &app,
        &state,
        &id,
        PipelineStage::Builder,
        LogLevel::Info,
        &format!("输出路径 {}", built.path.display()),
    );
    // 自检只出一条汇总日志：逐项明细进报告卡，逐项写日志等于用日志量换看不完的字
    if !checks.is_empty() {
        let bad: Vec<&CheckResult> = checks
            .iter()
            .filter(|c| c.status != CheckStatus::Pass)
            .collect();
        let msg = if bad.is_empty() {
            format!("构建自检 {} 项 · 全部通过", checks.len())
        } else {
            let first = bad.first().unwrap();
            format!(
                "构建自检 {} 项 · {} 项需关注：{} — {}",
                checks.len(),
                bad.len(),
                first.label,
                first.detail
            )
        };
        push_log(
            &app,
            &state,
            &id,
            PipelineStage::Builder,
            if bad.is_empty() { LogLevel::Info } else { LogLevel::Warn },
            &msg,
        );
    }
    // zip 已在输出目录生成，暂存目录即刻回收（文件本体留在 cache/files 供跨任务复用）
    let _ = std::fs::remove_dir_all(&staging_path);
    let finished = now_ms();
    {
        let mut inner = state.inner.lock().unwrap();
        let t = match inner.tasks.get_mut(&id) {
            Some(t) if matches!(t.status, TaskStatus::Queued | TaskStatus::Running) => t,
            // 已被取消：不写成功态，但实时条要收掉
            _ => {
                if let Some(t) = inner.tasks.get_mut(&id) {
                    t.activity = None;
                }
                return;
            }
        };
        t.status = TaskStatus::Success;
        t.progress = 100;
        t.stage = Some(PipelineStage::Builder);
        t.finished_at = Some(finished);
        t.activity = None;
        t.output_file_name = Some(final_name.clone());
        t.output_path = Some(built.path.clone());
        t.output_size_bytes = Some(built.size);
        push_line(
            t,
            log_line(
                PipelineStage::Builder,
                LogLevel::Info,
                &format!("打包完成 · {final_name}"),
            ),
        );
        let duration_sec = t
            .started_at
            .map(|s| ((finished - s) / 1000).max(1) as u64)
            .unwrap_or(1);
        let report = ConversionReport {
            task_id: id.clone(),
            output_file_name: final_name.clone(),
            output_size_bytes: built.size,
            duration_sec,
            removed: counts.remove,
            kept: counts.keep,
            added: counts.add,
            pending_review: review.clone(),
            options: t.options.clone(),
            file_count: built.entries as u32,
            generated_files: built.generated.clone(),
            reused_root_files: built.reused_root.clone(),
            start_jar: built.start_jar.clone(),
            installed: built.installed,
            checks,
            skipped_mods: skipped_mods.clone(),
        };
        emit_progress(&app, t);
        inner.reports.insert(id.clone(), report);
    }
    notify_done(&app, &id);
    save_tasks(&app, &state.inner.lock().unwrap());
}


fn map_download_error(app: &AppHandle, state: &Arc<AppState>, id: &str, e: &DownloadError) {
    // `detail` 是带 URL 与状态码的原句（只进「复制诊断信息」），`code` 才是界面上那句话的种类
    let code = e.net_code();
    let (title, detail, attempts) = match e {
        DownloadError::Failed {
            file_name,
            attempts,
            cause,
        } => (
            // attempts>0 才是真联网重试；包内/本地读失败不叫「下载失败」
            if *attempts > 0 {
                "联网下载失败".to_string()
            } else {
                "文件获取失败".to_string()
            },
            format!("{file_name} — {cause}"),
            Some(*attempts),
        ),
        DownloadError::NotFound(s) => ("依赖解析失败".to_string(), s.clone(), None),
        other => ("取件失败".to_string(), other.to_string(), None),
    };
    fail(
        app,
        state,
        id,
        TaskError {
            stage: PipelineStage::Downloader,
            title,
            detail,
            code,
            retryable: true,
            attempts,
            log_tail: None,
            exit_code: None,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use super::installer_stage::{describe_layout, install_done_line, install_task_error};

    fn installed(from_cache: bool, scripts: &[&str], jars: &[&str], secs: u64) -> Installed {
        Installed {
            dir: PathBuf::from("X:\\cache\\installs\\forge\\1.20.1-47.4.10"),
            from_cache,
            report: installer::InstallReport {
                files: 107,
                bytes: 158 * 1024 * 1024,
                elapsed: Duration::from_secs(secs),
                scripts: scripts.iter().map(|s| s.to_string()).collect(),
                jars: jars.iter().map(|s| s.to_string()).collect(),
            },
        }
    }

    /// 收场行：复用与真装的说法分开（前者一次进程都没起），耗时只出现在真装过的那条上
    #[test]
    fn install_done_line_separates_cache_hit_from_real_run() {
        let hit = install_done_line(&installed(true, &["run.bat", "run.sh"], &[], 0), "Forge 1.20.1-47.4.10");
        assert!(hit.starts_with("复用已装的 Forge 1.20.1-47.4.10"), "{hit}");
        assert!(hit.contains("未起进程"), "命中必须说明零进程，否则读起来像装完了");
        assert!(!hit.contains("用时"), "命中没有耗时可报");

        let real = install_done_line(&installed(false, &["run.bat", "run.sh"], &[], 226), "Forge 1.20.1-47.4.10");
        assert!(real.contains("本机安装完成") && real.contains("226.0 秒"), "{real}");
        assert!(real.contains("顶层 run.bat、run.sh"), "{real}");
    }

    /// 老 Forge（≤1.16）装完没有 run 脚本、只有顶层散 jar：这条日志就是第 6 步三分叉的判据，
    /// 说法必须与真实布局对上，不能把「没脚本」写成成功
    #[test]
    fn install_done_line_names_the_layout_it_found() {
        let v = |items: &[&str]| items.iter().map(|s| s.to_string()).collect::<Vec<String>>();
        let old = describe_layout(&v(&[]), &v(&["forge-1.16.5-36.2.39.jar", "minecraft_server.1.16.5.jar"]));
        assert!(old.starts_with("无 run 脚本，顶层散 jar "), "{old}");
        assert_eq!(describe_layout(&v(&["run.sh"]), &v(&["a.jar"])), "顶层 run.sh + a.jar");
        assert_eq!(describe_layout(&v(&[]), &v(&[])), "顶层既无 run 脚本也无散 jar");
    }

    /// 取消不算失败：错误卡只由 Failed 分支生成，取消走的是与取件同款的中止日志
    #[test]
    fn install_failures_are_retryable_and_keep_the_cause() {
        let e = install_task_error(&installer::InstallError::Failed {
            code: "1".into(),
            tail: "安装器报告成功 / There was an error during installation".into(),
        });
        assert_eq!(e.stage, PipelineStage::Installer);
        assert!(e.retryable, "装 JDK 之后点重试就该能过，不能判成死路");
        assert!(e.detail.contains("退出码 1"), "{}", e.detail);
        assert!(e.detail.contains("There was an error"), "安装器的尾巴要跟着进错误卡，否则只剩一句空话");

        let t = install_task_error(&installer::InstallError::Timeout { secs: 1800 });
        assert!(t.detail.contains("半成品已回收"), "{}", t.detail);
        // 安装器退出码不是 i32 口径（Windows/Unix 不同），不硬塞进 exit_code 槽
        assert_eq!(e.exit_code, None);
    }

    /// README 那句 eula 说明跟着让位规则走：勾了包内那份就没有「已生成」这回事，
    /// 按开关写的提醒会变成一句关于不存在文件的指导
    #[test]
    fn readme_eula_line_follows_the_keep_gate() {
        let counts = PlanCounts { remove: 0, keep: 0, add: 0 };
        let lines = |keeps: &[&str], agree: bool| {
            let files: Vec<String> = keeps.iter().map(|s| s.to_string()).collect();
            build_readme(&[], &counts, &[], LoaderKind::Fabric, &[], &files, agree, false, &[])
        };

        let kept = lines(&["eula.txt"], false);
        assert!(kept.iter().any(|l| l.contains("沿用包内那份")), "{kept:?}");
        assert!(
            !kept.iter().any(|l| l.contains("eula=false")),
            "让位了还催用户改 false，那一句指的不是包里这份文件：{kept:?}"
        );
        // 没勾 + 开关关：还是原来那句提醒
        assert!(lines(&[], false).iter().any(|l| l.contains("eula=false")));
        // 没勾 + 开关开：两句都不该出现
        assert!(!lines(&[], true).iter().any(|l| l.contains("eula.txt")));
    }

    /// 缺件闸门放行后，产物里就是没有这些模组 ⇒ 包里那张纸必须逐条点名，
    /// 否则服主日后按整合包的模组表来数，数出一堆「莫名消失」的模组
    #[test]
    fn readme_lists_the_skipped_mods() {
        let counts = PlanCounts { remove: 0, keep: 0, add: 0 };
        let lines = |skipped: &[&str]| {
            let names: Vec<String> = skipped.iter().map(|s| s.to_string()).collect();
            build_readme(&[], &counts, &[], LoaderKind::Forge, &[], &[], true, false, &names)
        };
        assert!(lines(&[]).iter().all(|l| !l.contains("缺少模组")), "没跳过就不提这一档");
        let got = lines(&["Mod A", "Mod B"]);
        let line = got.iter().find(|l| l.contains("缺少模组")).unwrap();
        assert!(line.contains("2 个"), "{line}");
        assert!(line.contains("Mod A") && line.contains("Mod B"), "{line}");
    }
}
