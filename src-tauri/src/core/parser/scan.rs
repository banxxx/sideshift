use super::curseforge::cf_rows;
use super::*;

/* ---------------- 按物理条目扫一遍（裸包启发式 / CF 清单+自带 jar） ---------------- */

/// 裸 zip 收录非模组文件时跳过的重量级/无意义顶层目录（启动器缓存、运行时产物）
pub(super) const ZIP_SKIP_TOP_DIRS: &[&str] = &[
    "assets",
    "libraries",
    "versions",
    "logs",
    "screenshots",
    "run",
    "runtime",
    "java",
    "bin",
    "natives",
    "downloads",
];

/// 逐条扫 zip 里的物理文件：`mods` 段定包根，jar 归模组、其余归「保留内容」。
///
/// 这条腿服务两种包：什么清单都没有的**裸 zip**（`declared` 全 None ⇒ 版本与加载器只能猜），
/// 以及 **CF/MCBBS 那种「清单只说家族与版本、jar 仍躺在 overrides/mods 里」的民间包**
/// （`declared` 有值 ⇒ 声明压过猜测）。
/// `no_jar_msg` 是「一条 jar 都没扫出来」时的实话：裸包和 CF 包的病因不同，不能共用一句
pub(super) fn scan_zip_entries(path: &Path, declared: &Declared, no_jar_msg: &str) -> Result<ParsedPack, String> {
    let zip = File::open(path).map_err(|e| format!("无法打开文件：{e}"))?;
    let mut archive = zip::ZipArchive::new(zip).map_err(|e| format!("zip 结构损坏：{e}"))?;

    let mut entries: Vec<(String, u64)> = Vec::new();
    for i in 0..archive.len() {
        let f = archive.by_index(i).map_err(|e| e.to_string())?;
        if !f.is_dir() {
            entries.push((f.name().to_string(), f.size()));
        }
    }
    entries.sort();

    // 允许包内容多一层根目录：先探测 mods 目录前缀。**官方 CF 包一条 jar 都没有**，
    // 探不到 mods 不能当场判死——那种包的模组是清单里的编号，字节得联网补
    let mods_prefix = detect_mods_prefix(&entries);

    let in_mods = |name: &str| match &mods_prefix {
        Some(ModsLayout::Prefix(p)) => name.starts_with(p.as_str()),
        Some(ModsLayout::Root) => !name.contains('/') && !name.contains('\\'),
        // 没有 mods 目录 ⇒ 包里不存在「算模组」的那一段，物理条目一律归保留内容
        None => false,
    };

    // 包根外层文件夹 = `mods/` 前缀里 `mods/` 之前那一段（`MyPack/mods/` → `MyPack/`；`mods/` → 空）。
    // 只探一次，只用于换算逻辑路径：条目物理路径原样留着，取件仍按它从 zip 里抽字节
    let root_prefix = match &mods_prefix {
        Some(ModsLayout::Prefix(p)) => p[..p.len() - "mods/".len()].to_string(),
        _ => String::new(),
    };

    let mut mod_files = Vec::new();
    let mut extra_files = Vec::new();
    for (name, size) in &entries {
        // 认下来的那份清单不进保留清单（mrpack 那条腿另有自己的跳过口径，这里只管 CF）
        if declared
            .index_entry
            .as_ref()
            .is_some_and(|idx| name.eq_ignore_ascii_case(idx))
        {
            continue;
        }
        let lower = name.to_lowercase();
        let file_name = Path::new(name)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        if lower.ends_with(".jar") && in_mods(name) {
            mod_files.push(PackFile {
                path: name.clone(),
                file_name,
                url: String::new(), // 裸包无下载源：downloader 将按名称在 Modrinth 反查
                sha1: None,
                size_bytes: *size,
                in_pack: true, // 裸 zip 的 jar 全部物理在包内
                env_server: None,
                env_client: None,
                depends: Vec::new(),
                cf: None,
            });
        } else if let Some(i) = lower.find('/') {
            // 非 mods 的目录文件全部收录（kubejs/地图/材质包等），供「客户端保留目录」卡勾选；
            // 启动器/运行时的重量级目录剔除，避免解析出上万条目
            let top = &lower[..i];
            // 第一段同样按段全等判（与 `detect_mods_prefix`、`keep_denied` 一条口径）：
            // `mods_backup/`、`mods_x/` 不是模组目录，里面的东西是「用户要不要留下的内容」，
            // 该进保留清单，而不是被 `starts_with("mods")` 悄悄丢掉
            if top != "mods" && !ZIP_SKIP_TOP_DIRS.contains(&top) {
                extra_files.push(PackFile {
                    path: name.clone(),
                    file_name,
                    url: String::new(),
                    sha1: None,
                    size_bytes: *size,
                    in_pack: true,
                    env_server: None,
                    env_client: None,
                    depends: Vec::new(),
                    cf: None,
                });
            }
        } else {
            // 根级散文件（`options.txt`、`servers.dat`）：与 mrpack 腿同一条口径，见
            // `parser::mrpack` 里那支同名分支。裸包这一层没有目录可判，不收就等于不给勾
            extra_files.push(PackFile {
                path: name.clone(),
                file_name,
                url: String::new(),
                sha1: None,
                size_bytes: *size,
                in_pack: true,
                env_server: None,
                env_client: None,
                depends: Vec::new(),
                cf: None,
            });
        }
    }

    if mod_files.is_empty() {
        // **有字节按字节、没字节才按编号**：CF 的 `files[]` 里连文件名都没有，离线状态下编号
        // 与包内 jar 对不上号（硬并会把同一个模组列两遍）。所以包里只要躺着 jar，`files[]` 就不参与；
        // 一条字节都没有（官方导出的 CF 包）才按编号出行，名字/大小/sha1/直链由 `core::cfpack` 联网补
        if !declared.files.is_empty() {
            mod_files = cf_rows(&declared.files);
        } else if mods_prefix.is_some() {
            return Err("mods 目录中没有任何 .jar 模组文件".into());
        } else {
            return Err(no_jar_msg.to_string());
        }
    }

    // 猜不到就留空串，不再兜 1.20.1：那一档会从「运行环境」下拉一路当真，带着 Java 需求线、
    // Loader 候选和模组反查建出一份作者没打算做的包。空值在界面露成「未识别」、在开始转换那道闸上停住，
    // 让用户自己挑一档（同「任何情况下都不主动添加模组」那条口径）
    let mc_version = declared
        .mc_version
        .clone()
        .unwrap_or_else(|| guess_mc_version(path, &entries).unwrap_or_default());
    // 声明与加载器本体优先；没有本体时读模组元数据，最后才用模组/包文件名的完整标记。
    let mod_jars: Vec<String> = mod_files.iter().map(|f| f.file_name.clone()).collect();
    let from_metadata = if declared.loader.is_none()
        && !mod_jars.iter().any(|name| loader_from_jar_name(name).is_some())
    {
        loader_from_mod_metadata(&mut archive, &mod_files)
    } else {
        None
    };
    let guessed = guess_loader(&mod_jars, from_metadata, path);
    // 声明压过猜测；但**只有猜出来的家族与声明同家**时，jar 文件名里那档版本号才配替补——
    // 家族都猜错了还拿它的版本去填，等于把 `fabric-loader-0.15.3.jar` 当成 forge 的版本用
    let from_jars = if declared.loader.is_none() || declared.loader == Some(guessed.0) {
        guessed.1
    } else {
        None
    };
    let loader = declared.loader.unwrap_or(guessed.0);
    let loader_version = declared.loader_version.clone().or(from_jars);

    let manifest = PackManifest {
        file_name: path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default(),
        loader,
        mc_version,
        mod_count: mod_files.len() as u32,
        size_bytes: std::fs::metadata(path).map(|m| m.len()).unwrap_or(0),
        parsed: true,
        error: None,
        // 裸 zip 的 jar 全在同一个文件里：env 取证层要按这个路径重开包读 jar 元数据
        source_path: Some(path.to_string_lossy().to_string()),
    };
    Ok(ParsedPack {
        manifest,
        mod_files,
        extra_files,
        // 裸 zip 的 dependencies 段是没有的，这一档来自 mods 里那枚加载器 jar 的文件名；
        // 认不出时仍为 None ⇒ 前端落到列表的推荐项（旧行为）
        loader_version,
        root_prefix,
        // 编号声明全收集一份带走：上面那道闸只决定「这次成不成行」，并不并由 `cfpack` 拿
        // 真实文件名对完号再说（混合形包那 222 枚就是这么丢的）
        declared_cf: declared.files.clone(),
    })
}

/// 找到直接存放 jar 的 mods 目录（兼容 "mods/" 或 "packname/mods/"）；
/// Root 表示 jar 直接散落在 zip 根目录的裸包
pub(super) enum ModsLayout {
    Prefix(String),
    Root,
}

/// mods 目录的判据：**「文件名所在那一段」全等于 `mods`**（大小写不敏感），不是路径里出现过 `mods/`。
/// 子串那条写法会把 `somemods/`、`SomePack_mods/` 这类目录当成模组目录，整包的模组与保留内容
/// 从此按一个不存在的目录分家——和 `keep_denied` 早就是段全等，两侧口径在这里对齐。
pub(super) fn detect_mods_prefix(entries: &[(String, u64)]) -> Option<ModsLayout> {
    for (name, _) in entries {
        let lower = name.replace('\\', "/").to_lowercase();
        if !lower.ends_with(".jar") {
            continue;
        }
        // 根级 jar（不含 `/`）留给下面那条 Root 兜底
        let Some((dir, _file)) = lower.rsplit_once('/') else {
            continue;
        };
        let seg_start = dir.rfind('/').map(|i| i + 1).unwrap_or(0);
        if &dir[seg_start..] != "mods" {
            continue;
        }
        // 前缀 = 「到这一段末尾的斜杠为止」，从**原样条目名**上切（`in_mods` 拿它跟原始路径比，
        // 大小写得跟着条目走）；偏移算在小写串上，极少数非 ASCII 大小写映射会变长时按 `get`
        // 跳过而不是越界
        if let Some(prefix) = name.get(..dir.len() + 1) {
            return Some(ModsLayout::Prefix(prefix.to_string()));
        }
    }
    // 兜底：根目录下直接放 jar 的裸包
    entries
        .iter()
        .any(|(n, _)| !n.contains('/') && !n.contains('\\') && n.to_lowercase().ends_with(".jar"))
        .then_some(ModsLayout::Root)
}

/// 从 zip 条目名/包文件名里找形如 1.20.1 / 1.21 的版本号
fn guess_mc_version(path: &Path, entries: &[(String, u64)]) -> Option<String> {
    for (name, _) in entries {
        let lower = name.to_lowercase();
        if lower.contains("minecraft") || lower.contains("mc_ver") || lower.contains("version.json")
        {
            if let Some(v) = first_version_like(&lower) {
                return Some(v);
            }
        }
    }
    let file = path.file_name()?.to_string_lossy().to_lowercase();
    first_version_like(&file)
}

pub(super) fn first_version_like(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'1' && bytes[i + 1] == b'.' && (i == 0 || !bytes[i - 1].is_ascii_digit()) {
            let mut j = i + 2;
            let mut segments = 1;
            while j < bytes.len() && (bytes[j].is_ascii_digit() || bytes[j] == b'.') {
                if bytes[j] == b'.' {
                    segments += 1;
                }
                j += 1;
            }
            if (2..=3).contains(&segments) {
                let cand = s[i..j].trim_end_matches('.');
                if cand.matches('.').count() >= 1 {
                    return Some(cand.to_string());
                }
            }
        }
        i += 1;
    }
    None
}

/// 加载器 jar 的文件名 ⇒（加载器，那一档版本号）。
/// 认的是官方构建产物的名字形状：`fabric-loader-0.16.9.jar`、
/// `neoforge-21.1.77-universal.jar`、`forge-1.20.1-47.2.20-universal.jar`。
/// 认不出版本一律给 None（下面那档自动选择会取推荐项），**不给一个像是版本的假串**。
pub(super) fn loader_from_jar_name(name: &str) -> Option<(LoaderKind, Option<String>)> {
    let lower = name.to_lowercase();
    let stem = lower.strip_suffix(".jar")?;
    // 本体版本必须紧跟前缀，后面只允许官方本体后缀或兼容旧包的 MC 版本段。
    // 不能跳过 helper/api 等段去找后面的数字：那会把普通模组当本体并绕过元数据。
    let parts = |rest: &str| -> Option<Vec<String>> {
        let segments: Vec<String> = rest.split('-').map(str::to_string).collect();
        if !segments.first().is_some_and(|s| is_dotted_num(s))
            || segments.iter().skip(1).any(|s| {
                !is_dotted_num(s) && !matches!(s.as_str(), "universal" | "installer")
            })
        {
            return None;
        }
        Some(segments)
    };
    if let Some(rest) = stem.strip_prefix("fabric-loader-") {
        let segments = parts(rest)?;
        Some((LoaderKind::Fabric, Some(segments[0].clone())))
    } else if let Some(rest) = stem.strip_prefix("neoforge-") {
        let segments = parts(rest)?;
        Some((LoaderKind::NeoForge, Some(segments[0].clone())))
    } else if let Some(rest) = stem.strip_prefix("forge-") {
        // Forge 是 `forge-<MC>-<构建>[-universal]`，也有包省掉 MC 那一段直接写构建号。
        // 两段都在时构建在后；只有一段且它像 MC 版本号（`1.` 开头）就不当构建号用
        let segments = parts(rest)?;
        let mut nums = segments.iter().filter(|s| is_dotted_num(s));
        let ver = match (nums.next(), nums.next()) {
            (Some(_mc), Some(build)) => Some(build.to_string()),
            (Some(only), None) if !only.starts_with("1.") => Some(only.to_string()),
            _ => None,
        };
        Some((LoaderKind::Forge, ver))
    } else {
        None
    }
}

fn is_dotted_num(s: &str) -> bool {
    s.contains('.')
        && s.split('.').all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
}

/// 裸 zip 的加载器判据：加载器本体 → 一致的模组元数据 → 模组/包文件名的完整标记。
/// 配置、资源与外层目录名不是加载器声明，不参与推断。无证据时仍保留原有 Fabric 兜底；
/// 元数据冲突时也交回文件名规则，待增加「未知/冲突」交互后再改变这条产品行为。
fn guess_loader(
    mod_jars: &[String],
    from_metadata: Option<LoaderKind>,
    path: &Path,
) -> (LoaderKind, Option<String>) {
    for jar in mod_jars {
        if let Some(hit) = loader_from_jar_name(jar) {
            return (hit.0, hit.1);
        }
    }
    if let Some(loader) = from_metadata {
        return (loader, None);
    }
    let has = |kw: &str| mod_jars.iter().any(|name| has_loader_marker(name, kw));
    // 多种弱标记冲突时沿用原顺序，避免这次修复同时改变冲突处理策略。
    if has("neoforge") {
        (LoaderKind::NeoForge, None)
    } else if has("forge") {
        (LoaderKind::Forge, None)
    } else if has("fabric") {
        (LoaderKind::Fabric, None)
    } else {
        let file = path
            .file_name()
            .map(|s| s.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if has_loader_marker(&file, "neoforge") {
            (LoaderKind::NeoForge, None)
        } else if has_loader_marker(&file, "forge") {
            (LoaderKind::Forge, None)
        } else {
            // 什么线索都没有时也算 Fabric（裸包默认，旧口径原样保留）
            (LoaderKind::Fabric, None)
        }
    }
}

fn has_loader_marker(name: &str, marker: &str) -> bool {
    name.split(|c: char| !c.is_ascii_alphanumeric())
        .any(|part| part.eq_ignore_ascii_case(marker))
}

/// 只读取模组 JAR 与其中的公开元数据，不解包到磁盘；单个容器/元数据设上限。
/// 多加载器 JAR 给出候选集合，以所有可识别 JAR 的交集确定唯一家族。
fn loader_from_mod_metadata(
    archive: &mut zip::ZipArchive<File>,
    files: &[PackFile],
) -> Option<LoaderKind> {
    const MAX_JAR_BYTES: u64 = 48 * 1024 * 1024;
    const MAX_SCAN_BYTES: u64 = 512 * 1024 * 1024;
    const MAX_SCAN_FILES: usize = 400;
    let mut common: Option<Vec<LoaderKind>> = None;
    let mut scanned_bytes = 0u64;
    for (index, file) in files.iter().filter(|f| f.in_pack).enumerate() {
        // 超出总预算就放弃本轮元数据结论；不把只扫描了部分文件的交集当成整包一致性。
        if index >= MAX_SCAN_FILES { return None; }
        let Ok(mut entry) = archive.by_name(&file.path) else { return None };
        if entry.size() > MAX_JAR_BYTES
            || entry.size() > MAX_SCAN_BYTES.saturating_sub(scanned_bytes)
        {
            return None;
        }
        let mut bytes = Vec::new();
        if (&mut entry).take(MAX_JAR_BYTES + 1).read_to_end(&mut bytes).is_err()
            || bytes.len() as u64 > MAX_JAR_BYTES
        {
            return None;
        }
        scanned_bytes = scanned_bytes.saturating_add(bytes.len() as u64);
        let hints = jar_loader_candidates(&bytes);
        if hints.is_empty() { continue; }
        match &mut common {
            None => common = Some(hints),
            Some(current) => current.retain(|kind| hints.contains(kind)),
        }
        // 不一致就交回原有弱规则，不用继续解剩余 JAR。
        if common.as_ref().is_some_and(Vec::is_empty) { return None; }
    }
    common.filter(|kinds| kinds.len() == 1).map(|kinds| kinds[0])
}

fn jar_loader_candidates(bytes: &[u8]) -> Vec<LoaderKind> {
    let Ok(mut jar) = zip::ZipArchive::new(std::io::Cursor::new(bytes)) else {
        return Vec::new();
    };
    let mut kinds = Vec::new();
    let has_neoforge = jar.by_name("META-INF/neoforge.mods.toml").is_ok();
    if has_neoforge { kinds.push(LoaderKind::NeoForge); }
    if jar.by_name("fabric.mod.json").is_ok() { kinds.push(LoaderKind::Fabric); }
    if jar.by_name("mcmod.info").is_ok() { kinds.push(LoaderKind::Forge); }
    if let Ok(mut meta) = jar.by_name("META-INF/mods.toml") {
        const MAX_META_BYTES: u64 = 1024 * 1024;
        let mut raw = String::new();
        let table = if meta.size() <= MAX_META_BYTES
            && (&mut meta).take(MAX_META_BYTES + 1).read_to_string(&mut raw).is_ok()
            && raw.len() as u64 <= MAX_META_BYTES
        {
            toml::from_str::<toml::Value>(&raw).ok()
        } else {
            None
        };
        let mut declared = Vec::new();
        if let Some(deps) = table.as_ref().and_then(|v| v.get("dependencies")).and_then(toml::Value::as_table) {
            for rows in deps.values().filter_map(toml::Value::as_array) {
                for row in rows {
                    let kind = match row.get("modId").and_then(toml::Value::as_str) {
                        Some("forge") => LoaderKind::Forge,
                        Some("neoforge") => LoaderKind::NeoForge,
                        _ => continue,
                    };
                    // 可选或不兼容的声明不能证明当前模组属于该加载器。
                    if row.get("mandatory").and_then(toml::Value::as_bool) == Some(false)
                        || row.get("type").and_then(toml::Value::as_str).is_some_and(|t| t != "required")
                    {
                        continue;
                    }
                    if !declared.contains(&kind) { declared.push(kind); }
                }
            }
        }
        // 旧 NeoForge 也使用 mods.toml；文件名本身只能证明 Forge 系，不能独自区分两家。
        if declared.is_empty() && !has_neoforge {
            declared.extend([LoaderKind::Forge, LoaderKind::NeoForge]);
        }
        for kind in declared {
            if !kinds.contains(&kind) { kinds.push(kind); }
        }
    }
    kinds
}

