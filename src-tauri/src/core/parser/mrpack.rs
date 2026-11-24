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

/// 服务端安装视角的逻辑路径及优先级（越小越先采用）。物理路径另外保留供 ZipEntry 取件。
fn server_logical_rel(root_prefix: &str, name: &str) -> Option<(String, u8)> {
    let normalized = name.replace('\\', "/");
    let rel = strip_root_prefix(root_prefix, &normalized);
    if !root_prefix.is_empty() && rel.len() == normalized.len() {
        return None;
    }
    let lower = rel.to_lowercase();
    if lower.starts_with("client-overrides/") {
        return None;
    }
    for (shell, priority) in [("server-overrides/", 0), ("overrides/", 1), ("override/", 2)] {
        if lower.starts_with(shell) {
            return Some((rel[shell.len()..].to_string(), priority));
        }
    }
    Some((rel.to_string(), 3))
}

/// mrpack 精确解析。`index_entry` 是清单在 zip 里的**物理条目名**，`root_prefix` 是它外面那层
/// 自定义文件夹（改名成 `.zip` 的 mrpack 套了一层壳时非空）。
/// 先按服务端覆盖规则合并物理文件，再按清单声明的实例相对路径补齐下载条目。
/// 路径保留实际入选的物理名，后续取件直接抽取该条目。
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

    // 先合成服务端唯一逻辑文件表，再和 index 合并。目录树、方案和取件共用同一份选择，
    // 不能分别收录两个覆盖层，否则后续防重名会把它们变成两枚实际加载的 JAR。
    let mut physical: BTreeMap<String, (u8, String, u64)> = BTreeMap::new();
    for i in 0..archive.len() {
        if let Ok(ent) = archive.by_index(i) {
            if ent.is_dir() {
                continue;
            }
            let Some((logical, priority)) = server_logical_rel(root_prefix, ent.name()) else {
                continue;
            };
            let key = logical.to_lowercase();
            if key == MRPACK_ENTRY {
                continue;
            }
            if physical.get(&key).is_none_or(|(old, _, _)| priority < *old) {
                physical.insert(key, (priority, ent.name().to_string(), ent.size()));
            }
        }
    }

    let mut mod_files = Vec::new();
    let mut extra_files = Vec::new();
    let mut declared = std::collections::HashSet::new();
    for f in &index.files {
        let norm_path = f.path.replace('\\', "/");
        let key = norm_path.to_lowercase();
        if !declared.insert(key.clone()) {
            continue;
        }
        let file_name = Path::new(&norm_path)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| norm_path.clone());
        // 声明路径相对实例根。服务端产物的覆盖优先级是：
        // `server-overrides/` > `overrides/`/`override/` > 包根；`client-overrides/` 只给客户端，
        // 不能拿它满足服务端的同一路径。
        let found = physical.remove(&key);
        let replaced = found.as_ref().is_some_and(|(priority, _, _)| *priority < 3);
        let path = found.as_ref()
            .map(|(_, name, _)| name.clone())
            .unwrap_or_else(|| format!("{root_prefix}{norm_path}"));
        // index 声明但物理缺失（残缺包）→ in_pack=false，构建时按 URL 补下载
        let entry_size = found.as_ref().map(|(_, _, size)| *size);
        // 覆盖的是字节身份，不只路径：旧下载地址、哈希、端声明和依赖不能附在新 JAR 上。
        // 覆盖文件的端与依赖由后续 JAR 取证重新读取，避免旧声明把服务端替换文件错误剔除。
        let env = if replaced { None } else { f.env.as_ref() };
        // downloads 为空 = 包内自带（local）文件：同样计入模组与方案，
        // 构建时由流水线经 Fetch::ZipEntry 直接从源包抽取，不联网
        let pf = PackFile {
            path,
            file_name,
            url: if replaced { String::new() } else { f.downloads.first().cloned().unwrap_or_default() },
            sha1: if replaced { None } else { f.hashes.get("sha1").cloned() },
            size_bytes: entry_size.or(f.file_size).unwrap_or(0),
            in_pack: entry_size.is_some(),
            env_server: env_flag(env.and_then(|e| e.server.as_deref())),
            env_client: env_flag(env.and_then(|e| e.client.as_deref())),
            depends: if replaced { Vec::new() } else { f
                .depends
                .iter()
                .filter(|d| !d.optional && !d.project_id.is_empty())
                .map(|d| d.project_id.clone())
                .collect() },
            // mrpack 的 files[] 自带 downloads，不需要 CF 那一套坐标
            cf: None,
        };
        // 判模组仍用**声明路径**（它才是实例根视角）；物理名带着外层壳，拿它判会整包漏认
        if key.starts_with("mods/") && key.ends_with(".jar") {
            mod_files.push(pf);
        } else {
            extra_files.push(pf);
        }
    }

    // 补收 mrpack 内「未在 index 声明」的物理文件。服务端覆盖优先级为
    // server-overrides > overrides/override > 包根；client-overrides 完全不进入服务端。
    for (lower, (_priority, name, size)) in physical {
        let file_name = Path::new(&name)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| name.clone());
        // 根级散文件（`options.txt`、`servers.dat` 这类）：没有目录层可判，直接按可保留内容收。
        // 这一支是格式壳剥掉之后才露出来的（`server-overrides/server.properties` → 包根文件）。
        let Some(j) = lower.find('/') else {
            extra_files.push(PackFile {
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
            continue;
        };
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
        // mrpack 的 `files[]` 每条都自带 downloads/字节，没有「只给编号」那一档 ⇒ 恒空
        declared_cf: Vec::new(),
    })
}

