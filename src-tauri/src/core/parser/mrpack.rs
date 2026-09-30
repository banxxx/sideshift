use super::scan::ZIP_SKIP_TOP_DIRS;
use super::*;

/* ---------------- mrpack ---------------- */

#[derive(Deserialize)]
struct IndexJson {
    #[serde(default)]
    dependencies: BTreeMap<String, String>,
    #[serde(default)]
    files: Vec<RawFile>,
}

#[derive(Deserialize)]
struct RawFile {
    #[serde(default)]
    path: String,
    #[serde(default)]
    hashes: BTreeMap<String, String>,
    #[serde(default)]
    downloads: Vec<String>,
    #[serde(default)]
    env: Option<RawEnv>,
    #[serde(default)]
    depends: Vec<RawDep>,
    #[serde(default, rename = "fileSize")]
    file_size: Option<u64>,
}

#[derive(Deserialize)]
struct RawEnv {
    server: Option<String>,
    #[serde(default)]
    client: Option<String>,
}

/// mrpack env 取值 → 支持度：规范只有 required/unsupported，见到别的值按可选待。
/// **缺键必须返回 None**（很多包整个 `env` 段都不写）：写成 Optional 会让下游以为
/// 「作者声明过」，既吃掉证据阶梯的后几层，也永远判不出剔除。
fn env_flag(v: Option<&str>) -> Option<SideFlag> {
    let raw = v?.to_lowercase();
    match raw.as_str() {
        "required" => Some(SideFlag::Required),
        "unsupported" => Some(SideFlag::Unsupported),
        _ => Some(SideFlag::Optional),
    }
}

/// mrpack files[].depends[]：指向包内另一 Modrinth 文件版本的项目引用
#[derive(Deserialize)]
struct RawDep {
    #[serde(default)]
    project_id: String,
    #[serde(default)]
    optional: bool,
}

fn detect_loader_from_deps(deps: &BTreeMap<String, String>) -> LoaderKind {
    if deps.contains_key("fabric-loader") {
        LoaderKind::Fabric
    } else if deps.contains_key("neoforge") {
        LoaderKind::NeoForge
    } else if deps.contains_key("forge") {
        LoaderKind::Forge
    } else {
        LoaderKind::Fabric
    }
}

/// mrpack 精确解析。`index_entry` 是清单在 zip 里的**物理条目名**，`root_prefix` 是它外面那层
/// 自定义文件夹（改名成 `.zip` 的 mrpack 套了一层壳时非空）。
/// 除此之外与老写法逐条一致：清单行仍按**声明路径**（相对实例根，不带壳）判 in_pack，
/// 物理缺失就照旧回落 URL。
pub(super) fn parse_mrpack(
    path: &Path,
    index_entry: &str,
    root_prefix: &str,
) -> Result<ParsedPack, String> {
    let zip = File::open(path).map_err(|e| format!("无法打开文件：{e}"))?;
    let mut archive =
        zip::ZipArchive::new(zip).map_err(|e| format!("zip 结构损坏：{e}"))?;

    let index: IndexJson = {
        let mut entry = archive
            .by_name(index_entry)
            .map_err(|_| NO_INDEX.to_string())?;
        let mut buf = Vec::new();
        entry
            .read_to_end(&mut buf)
            .map_err(|e| e.to_string())?;
        serde_json::from_slice(&buf)
            .map_err(|e| format!("modrinth.index.json 解析失败：{e}"))?
    };

    let loader = detect_loader_from_deps(&index.dependencies);
    let loader_version = match loader {
        LoaderKind::Fabric => index.dependencies.get("fabric-loader").cloned(),
        LoaderKind::Forge => index.dependencies.get("forge").cloned(),
        LoaderKind::NeoForge => index.dependencies.get("neoforge").cloned(),
    };
    // 顶层 game 字段按规范恒为 "minecraft"（游戏 ID），版本号在 dependencies.minecraft
    let mc_version = index
        .dependencies
        .get("minecraft")
        .cloned()
        .unwrap_or_default();

    // 物理条目表（条目名 → 解压后大小）：判 index 行 in_pack，兼作未声明文件的补收遍历
    let mut entry_sizes: BTreeMap<String, u64> = BTreeMap::new();
    for i in 0..archive.len() {
        if let Ok(ent) = archive.by_index(i) {
            if !ent.is_dir() {
                entry_sizes.insert(ent.name().to_string(), ent.size());
            }
        }
    }

    let mut mod_files = Vec::new();
    let mut extra_files = Vec::new();
    for f in &index.files {
        let norm_path = f.path.replace('\\', "/");
        let file_name = Path::new(&norm_path)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| norm_path.clone());
        let env = f.env.as_ref();
        // 声明路径相对**实例根**，包里的物理名有两副面孔：
        // `MyPack/mods/x.jar`（外层壳 + 原路径）与 `MyPack/overrides/mods/x.jar`（再套一层格式壳）——
        // 后者是民间 mrpack 的常态：jar 直接躺在 overrides 里，`files[]` 写的仍是实例根路径。
        // 只认前一副就把包内明明躺着的字节判成 in_pack=false ⇒ 转头联网重下，离线场景当场瘫
        let mut found: Option<(String, u64)> = None;
        for shell in ["", "overrides/", "override/"] {
            let cand = format!("{root_prefix}{shell}{norm_path}");
            if let Some(s) = entry_sizes.get(&cand).copied() {
                found = Some((cand, s));
                break;
            }
        }
        let physical = found
            .clone()
            .map(|(n, _)| n)
            .unwrap_or_else(|| format!("{root_prefix}{norm_path}"));
        // index 声明但物理缺失（残缺包）→ in_pack=false，构建时按 URL 补下载
        let entry_size = found.as_ref().map(|(_, s)| *s);
        // downloads 为空 = 包内自带（local）文件：同样计入模组与方案，
        // 构建时由流水线经 Fetch::ZipEntry 直接从源包抽取，不联网
        let pf = PackFile {
            path: physical,
            file_name,
            url: f.downloads.first().cloned().unwrap_or_default(),
            sha1: f.hashes.get("sha1").cloned(),
            size_bytes: entry_size.or(f.file_size).unwrap_or(0),
            in_pack: entry_size.is_some(),
            env_server: env_flag(env.and_then(|e| e.server.as_deref())),
            env_client: env_flag(env.and_then(|e| e.client.as_deref())),
            depends: f
                .depends
                .iter()
                .filter(|d| !d.optional && !d.project_id.is_empty())
                .map(|d| d.project_id.clone())
                .collect(),
            // mrpack 的 files[] 自带 downloads，不需要 CF 那一套坐标
            cf: None,
        };
        // 判模组仍用**声明路径**（它才是实例根视角）；物理名带着外层壳，拿它判会整包漏认
        if norm_path.starts_with("mods/") && pf.file_name.ends_with(".jar") {
            mod_files.push(pf);
        } else {
            extra_files.push(pf);
        }
    }

    // 补收 mrpack 内「未在 index 声明」的物理文件：手动拖进 zip 的 kubejs/、地图等
    // 目录不会出现在 modrinth.index.json 里，只能直接枚举 zip 条目拿到
    let declared: std::collections::HashSet<String> = index
        .files
        .iter()
        .map(|f| f.path.replace('\\', "/"))
        .collect();
    for (name, size) in entry_sizes {
        // 判据一律走「剥掉外层文件夹**与**格式壳」的那份：声明路径、`mods/` 段名、重量级目录名单
        // 都是实例根视角。只剥外层的话，躺在 `overrides/mods/` 里的未声明 jar 首段是 `overrides`，
        // 会被当普通保留内容收走，再由 `keep_denied` 在构建层一口吃掉 ⇒ 这批模组整个从方案里消失
        let logical = logical_rel(strip_root_prefix(root_prefix, &name));
        let lower = logical.to_lowercase();
        if lower == MRPACK_ENTRY || declared.contains(logical) {
            continue;
        }
        let file_name = Path::new(&name)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| name.clone());
        // 只认「某目录下的文件」，且跳过启动器重量级目录
        let Some(j) = lower.find('/') else { continue };
        let top = &lower[..j];
        if top == "mods" {
            // 手动塞进 mrpack /mods 的未声明 jar：mrpack 规范允许，启动器原样安装，
            // 方案必须收录（无 url 无 sha1 → 构建时 ZipEntry 直取）；其余文件忽略
            if lower.ends_with(".jar") {
                mod_files.push(PackFile {
                    path: name,
                    file_name,
                    url: String::new(),
                    sha1: None,
                    size_bytes: size,
                    in_pack: true,
                    env_server: None,
                    env_client: None,
                    depends: Vec::new(),
                    cf: None,
                });
            }
            continue;
        }
        if ZIP_SKIP_TOP_DIRS.contains(&top) {
            continue;
        }
        extra_files.push(PackFile {
            path: name,
            file_name,
            url: String::new(), // 物理存在于源包：构建时 Fetch::ZipEntry 直接抽取
            sha1: None,
            size_bytes: size,
            in_pack: true,
            env_server: None,
            env_client: None,
            depends: Vec::new(),
            cf: None,
        });
    }

    let mut error = None;
    if mc_version.is_empty() {
        error = Some("modrinth.index.json 缺少 dependencies.minecraft（无法确定 Minecraft 版本）".into());
    } else if mod_files.is_empty() {
        error = Some("整合包中没有任何模组文件（mods 目录为空）".into());
    }
    let manifest = PackManifest {
        file_name: path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default(),
        loader,
        mc_version,
        mod_count: mod_files.len() as u32,
        size_bytes: std::fs::metadata(path).map(|m| m.len()).unwrap_or(0),
        parsed: error.is_none(),
        error,
        source_path: None,
    };
    Ok(ParsedPack {
        manifest,
        mod_files,
        extra_files,
        loader_version,
        // 清单外面那层自定义文件夹（`.zip` 套壳的 mrpack 才有；正经 mrpack 恒空）。
        // CF 那层 `overrides/` 不在此列，由 logical_rel 剥
        root_prefix: root_prefix.to_string(),
    })
}

