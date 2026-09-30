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
    // mods 里那枚加载器 jar 优先（顺带取版本号），没有再退到整包字样那一档
    let mod_jars: Vec<String> = mod_files.iter().map(|f| f.file_name.clone()).collect();
    let guessed = guess_loader(&mod_jars, &entries, path);
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
    // 名字里第一段「点分数字」就是版本号
    let first_seg = |rest: &str| {
        rest.split('-')
            .find(|s| is_dotted_num(s))
            .map(|s| s.to_string())
    };
    if let Some(rest) = stem.strip_prefix("fabric-loader-") {
        Some((LoaderKind::Fabric, first_seg(rest)))
    } else if let Some(rest) = stem.strip_prefix("neoforge-") {
        Some((LoaderKind::NeoForge, first_seg(rest)))
    } else if let Some(rest) = stem.strip_prefix("forge-") {
        // Forge 是 `forge-<MC>-<构建>[-universal]`，也有包省掉 MC 那一段直接写构建号。
        // 两段都在时构建在后；只有一段且它像 MC 版本号（`1.` 开头）就不当构建号用
        let mut nums = rest.split('-').filter(|s| is_dotted_num(s));
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
    // `contains('.')` 已经排掉空串：空串的 `all()` 会返回 true，不能当版本号
    s.contains('.') && s.chars().all(|c| c.is_ascii_digit() || c == '.')
}

/// 裸 zip 的加载器判据，两档。
/// **强证据**：mods 目录里那枚加载器 jar 的文件名——唯一能同时给出「哪一家」和「哪一档」的来源，
/// 版本号因此不再恒为空（以前这一档只能让前端落到候选列表的推荐项上）。
/// **弱证据**（原样保留的旧写法）：整包条目名/包文件名里出现过 `neoforge`/`forge`/`fabric` 字样。
/// 留着它是因为老 Forge（1.16 及以下）的包 mods 里本来就没有加载器 jar，删了这档会把整批包
/// 统一误标成 Fabric；代价是一条 `config/forgeconfigmodifier.cfg` 也能定这一家，所以它不配给版本号。
/// Quilt 等第四家仍落 Fabric（LoaderKind 只有三档，那条遗留未在本轮改判）。
fn guess_loader(
    mod_jars: &[String],
    entries: &[(String, u64)],
    path: &Path,
) -> (LoaderKind, Option<String>) {
    for jar in mod_jars {
        if let Some(hit) = loader_from_jar_name(jar) {
            return (hit.0, hit.1);
        }
    }
    let has = |kw: &str| entries.iter().any(|(n, _)| n.to_lowercase().contains(kw));
    // 弱证据那一档与旧写法逐条一致（条目名优先，全都不沾时看包文件名，最后落 Fabric）——
    // 本轮只把「mods 里那枚 jar 直取」加在它前面，不顺手改判这一条
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
        if file.contains("neoforge") {
            (LoaderKind::NeoForge, None)
        } else if file.contains("forge") {
            (LoaderKind::Forge, None)
        } else {
            // 什么线索都没有时也算 Fabric（裸包默认，旧口径原样保留）
            (LoaderKind::Fabric, None)
        }
    }
}

