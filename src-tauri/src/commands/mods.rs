use super::*;

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

