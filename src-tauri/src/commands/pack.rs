use super::*;

/* ---------------- 解析 / 选项 ---------------- */

/// 取证结论作废。每次**从磁盘重解一个包**都要走这里，连「身份没变的同一枚文件」也不例外：
/// 原地重导出的包路径一模一样，按身份比对认不出它换过血，复用旧取证就是「分类数据是上一个包的」。
/// 代价只有离线 jar 扫描重跑（磁盘端索引照常命中、联网层不重发），切页往返不重解析所以照常复用。
/// 旧分类轮以解析序号校验归属；作废时可以清运行标记，旧轮不得再清理新轮。
fn invalidate_env(inner: &mut task_engine::Inner) {
    inner.env_evidence.clear();
    inner.env_code.clear();
    inner.env_meta.clear();
    inner.env_doubt.clear();
    inner.env_evidence_pack = None;
    inner.env_online_pack = None;
}

fn begin_parse(inner: &mut task_engine::Inner) -> u64 {
    inner.parse_request_seq = inner.parse_request_seq.wrapping_add(1);
    inner.last_pack = None;
    invalidate_env(inner);
    inner.parse_request_seq
}

fn publish_parsed(inner: &mut task_engine::Inner, request: u64, parsed: Arc<ParsedPack>) -> bool {
    if inner.parse_request_seq != request {
        return false;
    }
    let id = parsed.manifest.identity();
    inner.parsed_by_pack.insert(id.clone(), parsed);
    inner.last_pack = Some(id);
    invalidate_env(inner);
    true
}

#[tauri::command]
pub async fn parse_pack(state: S<'_>, path: String) -> Result<PackManifest, String> {
    let request = begin_parse(&mut lock(&state));
    let source = PathBuf::from(&path);
    let work_path = source.clone();
    // 裸包需要读取嵌套 JAR 元数据，文件 I/O 与解压不能占住 Tauri 命令线程。
    let parsed = match tauri::async_runtime::spawn_blocking(move || parser::parse(&work_path)).await {
        Ok(parsed) => Arc::new(parsed),
        Err(_) => {
            let mut inner = lock(&state);
            if inner.parse_request_seq == request {
                inner.last_pack = None;
                invalidate_env(&mut inner);
            }
            return Ok(PackManifest {
                file_name: source.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
                loader: LoaderKind::Fabric,
                mc_version: String::new(),
                mod_count: 0,
                size_bytes: 0,
                parsed: false,
                error: Some("整合包解析任务意外中断，请重新选择文件".into()),
                source_path: Some(path),
            });
        }
    };
    let manifest = parsed.manifest.clone();
    // 旧请求仍返回原来的载荷形状，由前端序号过滤，但不能改后端当前包或其证据。
    publish_parsed(&mut lock(&state), request, parsed);
    Ok(manifest)
}

/// 历史任务只读查询：确保指定包有解析缓存，不切换当前选包或作废当前分类。
#[tauri::command]
pub async fn ensure_parsed(state: S<'_>, manifest: PackManifest) -> Result<bool, String> {
    let id = manifest.identity();
    {
        let inner = lock(&state);
        if inner.parsed_by_pack.contains_key(&id) {
            return Ok(true);
        }
    }

    let Some(src) = manifest.source_path.as_deref() else {
        return Ok(false);
    };
    let path = PathBuf::from(src);
    if !path.exists() {
        return Ok(false);
    }
    // 解析在锁外：几百个 jar 的包读到这里要是还握着全局锁，别的命令全跟着排队
    let parsed = Arc::new(tauri::async_runtime::spawn_blocking(move || parser::parse(&path))
        .await.map_err(|_| app_code("panic"))?);
    Ok(cache_history_parsed(&mut lock(&state), id, parsed))
}

fn cache_history_parsed(inner: &mut task_engine::Inner, id: String, parsed: Arc<ParsedPack>) -> bool {
    if !parsed.manifest.parsed { return false; }
    // 此期间用户可能重新选中同一路径；只补空缺，不覆盖刚发布的新内容。
    inner.parsed_by_pack.entry(id).or_insert(parsed);
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
    pack_id: Option<String>,
) -> Result<Vec<VersionOption>, String> {
    let loader = parsed_for_query(&lock(&state), pack_id.as_deref())
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
        .map_err(|_| app_code("panic"))
}

/// 某 MC 版本的 Java 需求线（Rust: `java_requirement`）。给转换页换版本时改写方案用：
/// 这张表只在 Rust 一份，前端复刻一份就会跟实跑的那把筛子走偏。
/// 为什么由前端改写而不是消费方现算：`options.java_version` 是**快照字段**，回看与重试读的都是
/// 当时那一档；在报告或实跑里现算会让旧任务被新表改写。
///
/// 两腿取数，顺序不能反：
/// 1. 官方 `javaVersion.majorVersion`（`Downloader::java_major_official`，命中本地表零请求）；
/// 2. 取不到才回落到 `core::java::required_for_mc` 那张表 —— 离线转换、BMCLAPI 没这一条腿的镜像、
///    清单里没这个号（自造版本号）三类情形都得有答案。
/// 表本身按官方实测补齐了 26.x（=Java 25），所以第二腿今天与第一腿同结论；第一腿的价值是
/// Mojang 下次抬档时不用再发一版。
///
/// `default_options` 那一头仍走表（同步、首帧就要有值）：这条命令是它之后到的那一次覆盖，
/// 值相同前端就不 patch，也就不会白重探一趟 Java。
///
/// 返回类型是 `Result` 而不是 `String`：Tauri 的 async 命令有这条约束。这里**永远 Ok**——
/// 官方字段取不到就是回落，没有值得报错的分支（报出去反而会在界面上多一句没必要的红字）。
#[tauri::command]
pub async fn java_requirement(state: S<'_>, mc_version: String) -> Result<String, String> {
    let cache_dir = PathBuf::from(lock(&state).settings.cache_dir.clone());
    Ok(
        match downloader_of(&state)
            .java_major_official(&cache_dir, &mc_version)
            .await
        {
            Some(major) => major.to_string(),
            None => java::required_for_mc(&mc_version).to_string(),
        },
    )
}

#[tauri::command]
pub fn default_options(state: S<'_>, manifest: PackManifest) -> ConversionOptions {
    let (loader_version, install_loader_locally) = {
        let inner = lock(&state);
        (
            inner
                .parsed_by_pack
                .get(&manifest.identity())
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

/// 最近一次解析包内可保留的内容：目录树 + 根级散文件
/// （mods 之外；文件数与体积递归统计、直属文件另存一层，同层按名升序）
#[tauri::command]
pub fn list_pack_dirs(state: S<'_>, pack_id: Option<String>) -> PackDirTree {
    let Some(p) = parsed_for_query(&lock(&state), pack_id.as_deref()) else {
        return PackDirTree::default();
    };

    #[derive(Default)]
    struct Node {
        counts: u32,
        bytes: u64,
        files: Vec<PackFileNode>,
        children: std::collections::BTreeMap<String, Node>,
    }
    fn build(name: &str, node: &mut Node) -> PackDirNode {
        let mut files = std::mem::take(&mut node.files);
        // extra_files 的顺序不是名字序，这一层要自己排（子目录靠 BTreeMap 天然有序）
        files.sort_by(|a, b| a.name.cmp(&b.name));
        PackDirNode {
            name: name.to_string(),
            file_count: node.counts,
            size_bytes: node.bytes,
            files,
            children: node
                .children
                .iter_mut()
                .map(|(n, c)| build(n, c))
                .collect(),
        }
    }

    let mut root = Node::default();
    // mods 由「模组方案」卡管理；resourcepacks 是客户端资源，服务端不消费——两族都不进保留树
    // （判据单源在 parser::KEEP_SKIP_TOP，看的是路径任一段，取件与预估两条腿共用；
    //  落位是「勾哪层剪哪层」，所以 `config/mods` 也必须拦，否则它落出来就是包根 mods/）；
    // overrides/ 壳前缀剥离后再入树（CF 格式内容映射到包根，与拷贝口径一致）
    let mut root_files: Vec<PackFileNode> = Vec::new();
    for f in &p.extra_files {
        let rel = f.path.replace('\\', "/");
        let logical = p.logical_rel(&rel);
        let lower = logical.to_lowercase();
        let segs: Vec<&str> = lower.split('/').collect();
        if parser::keep_denied(&lower) {
            continue;
        }
        let dirs = &segs[..segs.len() - 1];
        let node = PackFileNode {
            name: segs[segs.len() - 1].to_string(),
            size_bytes: f.size_bytes,
            // 与目录节点同一口径：树上的路径一律小写逻辑路径，勾选值直接拿它当 key
            path: lower.clone(),
        };
        if dirs.is_empty() {
            // 根级散文件（`options.txt`、`servers.dat` 这类）：旧代码里 `segs.len() < 2` 一句
            // 直接丢弃 ⇒ 既看不到也勾不走。现在单列成 files，勾选走精确全等（见 keep_files）
            root_files.push(node);
            continue;
        }
        let mut cur = &mut root;
        // 先下行再计数：这条文件要算进**每一层祖先目录**，包括它自己所在的那一层。
        // 旧写法是「计数→下行」，最深那层永远拿不到自己的直属文件 ⇒ 一条只放在 `config` 根上的
        // 文件让 config 报 0，而弹窗现在会把直属文件列出来，读数与列表当场对不上
        for seg in dirs {
            cur = cur.children.entry((*seg).to_string()).or_default();
            cur.counts += 1;
            cur.bytes += f.size_bytes;
        }
        cur.files.push(node);
    }
    root_files.sort_by(|a, b| a.name.cmp(&b.name));

    PackDirTree {
        dirs: root
            .children
            .iter_mut()
            .map(|(n, c)| build(n, c))
            .collect(),
        files: root_files,
    }
}

fn parsed_for_query(inner: &task_engine::Inner, pack_id: Option<&str>) -> Option<Arc<ParsedPack>> {
    let id = pack_id.or(inner.last_pack.as_deref())?;
    inner.parsed_by_pack.get(id).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_pack(path: &str, loader: LoaderKind) -> Arc<ParsedPack> {
        Arc::new(ParsedPack {
            manifest: PackManifest {
                file_name: "fixture.zip".into(),
                loader,
                mc_version: "1.20.1".into(),
                mod_count: 0,
                size_bytes: 0,
                parsed: true,
                error: None,
                source_path: Some(path.into()),
            },
            mod_files: Vec::new(),
            extra_files: Vec::new(),
            loader_version: None,
            root_prefix: String::new(),
            declared_cf: Vec::new(),
        })
    }

    #[test]
    fn late_parse_result_cannot_replace_the_last_selected_pack() {
        let mut inner = task_engine::Inner::default();
        let old = begin_parse(&mut inner);
        let latest = begin_parse(&mut inner);
        assert!(publish_parsed(&mut inner, latest, synthetic_pack("fixture-b.zip", LoaderKind::Forge)));
        inner.env_evidence_pack = Some("fixture-b.zip".into());
        assert!(!publish_parsed(&mut inner, old, synthetic_pack("fixture-a.zip", LoaderKind::NeoForge)));
        assert_eq!(inner.last_pack.as_deref(), Some("fixture-b.zip"));
        assert_eq!(inner.env_evidence_pack.as_deref(), Some("fixture-b.zip"));
        assert!(!inner.parsed_by_pack.contains_key("fixture-a.zip"));
    }

    #[test]
    fn repeated_path_still_requires_the_latest_parse_generation() {
        let mut inner = task_engine::Inner::default();
        let old = begin_parse(&mut inner);
        let latest = begin_parse(&mut inner);
        assert!(publish_parsed(&mut inner, latest, synthetic_pack("fixture.zip", LoaderKind::Forge)));
        assert!(!publish_parsed(&mut inner, old, synthetic_pack("fixture.zip", LoaderKind::NeoForge)));
        assert_eq!(inner.parsed_by_pack["fixture.zip"].manifest.loader, LoaderKind::Forge);
    }

    #[test]
    fn historical_cache_and_explicit_queries_leave_the_current_pack_untouched() {
        let mut inner = task_engine::Inner::default();
        let request = begin_parse(&mut inner);
        assert!(publish_parsed(&mut inner, request, synthetic_pack("current.zip", LoaderKind::Forge)));
        inner.env_evidence_pack = Some("current.zip".into());
        inner.env_online_pack = Some("current.zip".into());
        inner.classify_request_seq = 7;
        let parsed_history = synthetic_pack("history.zip", LoaderKind::NeoForge);
        assert!(cache_history_parsed(&mut inner, "history.zip".into(), parsed_history));
        assert_eq!(parsed_for_query(&inner, Some("history.zip")).unwrap().manifest.loader, LoaderKind::NeoForge);
        assert_eq!(parsed_for_query(&inner, None).unwrap().manifest.loader, LoaderKind::Forge);
        assert!(parsed_for_query(&inner, Some("missing.zip")).is_none());
        // 回看同一路径的旧结果不能盖掉首页后来发布的新内容。
        assert!(cache_history_parsed(&mut inner, "current.zip".into(), synthetic_pack("current.zip", LoaderKind::NeoForge)));
        assert_eq!(parsed_for_query(&inner, Some("current.zip")).unwrap().manifest.loader, LoaderKind::Forge);
        assert_eq!(inner.last_pack.as_deref(), Some("current.zip"));
        assert_eq!(inner.env_evidence_pack.as_deref(), Some("current.zip"));
        assert_eq!(inner.env_online_pack.as_deref(), Some("current.zip"));
        assert_eq!(inner.parse_request_seq, request);
        assert_eq!(inner.classify_request_seq, 7);
    }
}

