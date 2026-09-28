//! 整合包解析：.mrpack 精确解析（modrinth.index.json）；裸 .zip 启发式扫描 mods/ 目录。
//!
//! mrpack 规范口径：顶层 `game` 恒为游戏 ID（"minecraft"），MC 版本在 `dependencies.minecraft`；
//! `files[]` 条目应全部物理内嵌于 zip——以 zip 条目实测判定 `in_pack`，缺字节的残缺条目才回落 URL 下载。

use std::collections::BTreeMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use serde::Deserialize;

use crate::models::{LoaderKind, PackManifest, SideFlag};

/// mrpack 中单个文件条目（spec: modrinth.index.json）
#[derive(Debug, Clone)]
pub struct PackFile {
    /// 相对路径，如 mods/fabric-api.jar
    pub path: String,
    pub file_name: String,
    /// 下载 URL（mrpack 内嵌；裸 zip 与包内自带文件为空，需反查或直接抽取）
    pub url: String,
    /// files[].hashes.sha1，用作缓存键
    pub sha1: Option<String>,
    /// 原始文件大小（字节）：zip 条目实测大小优先，缺失时 index.fileSize；0 = 未知
    pub size_bytes: u64,
    /// 物理条目是否在源 zip 内（mrpack 规范要求 index 文件全部内嵌）。
    /// true = 构建时 ZipEntry 直取、不产生网络流量；false = index 声明了但包里没有，需按 URL 补下
    pub in_pack: bool,
    /// env.server 声明（mrpack 规范取值 required/unsupported）；None = 条目没写 env 段
    pub env_server: Option<SideFlag>,
    /// env.client 声明，同上
    pub env_client: Option<SideFlag>,
    /// 非可选依赖的 project_id 列表（mrpack files[].depends；裸 zip 为空）
    pub depends: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ParsedPack {
    pub manifest: PackManifest,
    /// mods/ 目录下的模组 jar（含 URL/env 信息，供 detector/downloader 消费）
    pub mod_files: Vec<PackFile>,
    /// 非 mods 资源文件（config 等）
    pub extra_files: Vec<PackFile>,
    /// mrpack 的 dependencies 段（minecraft/fabric-loader 版本）
    pub loader_version: Option<String>,
    /// 裸 zip 的「包根外层文件夹」（从 `MyPack/mods/` 那枚 jar 探出来的 `MyPack/`）；mrpack 与 CF 恒空。
    /// 逻辑路径换算要先剥它——不然勾选值、保留树、落位、预估全都会多出这一层，
    /// 而服务端按实例根读 `config/`，带着一层自定义目录的产物根本读不到
    pub root_prefix: String,
}

impl ParsedPack {
    /// 条目物理路径 → 交付包逻辑路径：先剥包根外层文件夹，再剥 `overrides/` 壳。
    /// 保留树 / 勾选键 / 落位 / 预估四条腿都走这一个入口，判据不会漂
    /// （`overrides/` 是格式壳、`root_prefix` 是打包习惯，两件事各剥一次，顺序固定）
    pub fn logical_rel<'a>(&self, rel: &'a str) -> &'a str {
        logical_rel(strip_root_prefix(&self.root_prefix, rel))
    }
}

/// 大小写不敏感地剥掉外层文件夹前缀（前缀与条目路径来自同一个 zip，正常同 case；混合大小的包靠这条兜住）。
/// 边界安全：前缀恒以 `/` 结尾，切点是 ASCII 字节
fn strip_root_prefix<'a>(prefix: &str, rel: &'a str) -> &'a str {
    if prefix.is_empty() {
        return rel;
    }
    let p = prefix.as_bytes();
    if rel.len() >= p.len() && rel.as_bytes()[..p.len()].eq_ignore_ascii_case(p) {
        return &rel[p.len()..];
    }
    rel
}

pub const MRPACK_ENTRY: &str = "modrinth.index.json";

/// CurseForge 惯例逻辑路径：`overrides/`（或 `override/`）只是格式外壳，其内容映射到包根。
/// 保留目录树与拷贝都按逻辑路径匹配，避免同一目录以「壳内/壳外」两种路径重复出现。
pub fn logical_rel(rel: &str) -> &str {
    let lower = rel.to_lowercase();
    for shell in ["overrides/", "override/"] {
        if lower.starts_with(shell) {
            return &rel[shell.len()..];
        }
    }
    rel
}

/// 保留范围之外的目录名：`mods` 由「模组方案」卡逐条决策，`resourcepacks` 是客户端资源、
/// 服务端不消费。**显示层（保留树）与取件层（构建 3.2 / 预估 3.2）共用这一条判据**——
/// 只在显示层挡等于给数据层留后门：旧草稿或手改存档里的一条 `mods` 就能把模组整棵复制进服务端。
pub const KEEP_SKIP_TOP: &[&str] = &["mods", "resourcepacks"];

/// 路径里**任一段**叫这两个名字 ⇒ 不在保留范围内（目录档与文件档同一判据）。
///
/// 为什么不只看首段：落位规则是「勾哪一层就把那一层剪到包根」（见 `kept_rel`），
/// 于是 `config/mods` 勾上之后落位就是包根 `mods/`，和直接勾 `mods` 是同一件事。
/// 外层壳的名字还不固定（`MyPack/mods` 这类压根不在首段上），按首段拦等于没拦。
/// 段与段全等，所以 `mods_backup` 不算 `mods`。
pub fn keep_denied(path: &str) -> bool {
    path.to_lowercase()
        .split('/')
        .any(|seg| KEEP_SKIP_TOP.contains(&seg))
}

/// 相对路径的落位名（最后一段）：勾选键是小写逻辑路径，而落位用的是条目自身的名字，
/// 所以「这条勾上去会叫什么」在两测都只能问这枚函数
pub fn base_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// 剪层落位：勾选键有 k 段 ⇒ 交付路径从原始逻辑路径的第 k 段（1 基）起算，
/// 也就是「勾的那一层挂到包根，它自己的内部层级原样保留」。大小写跟着条目原样走，不跟勾选键。
///
/// `config/mods` + `config/mods/fabric/a.jar` → `mods/fabric/a.jar`；
/// `kubejs/startup.js` → `startup.js`；顶层勾选（k=1）恒等，旧勾选值的行为不变。
/// 返回空串表示勾选键与条目路径段数不匹配（匹配逻辑出错），调用方必须跳过这条而不是落到包根。
pub fn kept_rel(pick: &str, logical: &str) -> String {
    let k = pick.split('/').count();
    let segs: Vec<&str> = logical.split('/').collect();
    if k == 0 || k > segs.len() {
        return String::new();
    }
    segs[k - 1..].join("/")
}

/// 解析入口：按扩展名分派；任何失败都返回 parsed:false 的 manifest（不 panic）
pub fn parse(path: &Path) -> ParsedPack {
    let file_name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let size_bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let source_path = path.to_string_lossy().to_string();

    let result = (|| -> Result<ParsedPack, String> {
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        match ext.as_str() {
            "mrpack" => parse_mrpack(path),
            "zip" => parse_plain_zip(path),
            "7z" => Err("暂不支持 .7z 格式，请先解压为 zip 或改用 .mrpack".into()),
            _ => Err(format!(
                "不支持的包格式：{}（仅支持 .mrpack / .zip）",
                file_name
            )),
        }
    })();

    match result {
        Ok(mut p) => {
            p.manifest.source_path = Some(source_path);
            p
        }
        Err(error) => ParsedPack {
            manifest: PackManifest {
                file_name,
                loader: LoaderKind::Fabric,
                mc_version: String::new(),
                mod_count: 0,
                size_bytes,
                parsed: false,
                error: Some(error),
                source_path: Some(source_path),
            },
            mod_files: Vec::new(),
            extra_files: Vec::new(),
            loader_version: None,
            root_prefix: String::new(),
        },
    }
}

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

fn parse_mrpack(path: &Path) -> Result<ParsedPack, String> {
    let zip = File::open(path).map_err(|e| format!("无法打开文件：{e}"))?;
    let mut archive =
        zip::ZipArchive::new(zip).map_err(|e| format!("zip 结构损坏：{e}"))?;

    let index: IndexJson = {
        let mut entry = archive
            .by_name(MRPACK_ENTRY)
            .map_err(|_| "缺少 modrinth.index.json，不是有效的 Modrinth 整合包".to_string())?;
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
        // index 声明但物理缺失（残缺包）→ in_pack=false，构建时按 URL 补下载
        let entry_size = entry_sizes.get(&norm_path).copied();
        // downloads 为空 = 包内自带（local）文件：同样计入模组与方案，
        // 构建时由流水线经 Fetch::ZipEntry 直接从源包抽取，不联网
        let pf = PackFile {
            path: norm_path,
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
        };
        if pf.path.starts_with("mods/") && pf.file_name.ends_with(".jar") {
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
        let lower = name.to_lowercase();
        if lower == MRPACK_ENTRY || declared.contains(&name) {
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
        // mrpack 的路径本身就相对实例根，没有外层文件夹可剥（CF 那层是 overrides/，由 logical_rel 剥）
        root_prefix: String::new(),
    })
}

/* ---------------- 裸 zip（启发式） ---------------- */

/// 裸 zip 收录非模组文件时跳过的重量级/无意义顶层目录（启动器缓存、运行时产物）
const ZIP_SKIP_TOP_DIRS: &[&str] = &[
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

fn parse_plain_zip(path: &Path) -> Result<ParsedPack, String> {
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

    // 允许包内容多一层根目录：先探测 mods 目录前缀
    let mods_prefix = detect_mods_prefix(&entries).ok_or(
        "未找到 mods 目录，不是可识别的整合包（推荐直接使用 .mrpack）",
    )?;

    let in_mods = |name: &str| match &mods_prefix {
        ModsLayout::Prefix(p) => name.starts_with(p.as_str()),
        ModsLayout::Root => !name.contains('/') && !name.contains('\\'),
    };

    // 包根外层文件夹 = `mods/` 前缀里 `mods/` 之前那一段（`MyPack/mods/` → `MyPack/`；`mods/` → 空）。
    // 只探一次，只用于换算逻辑路径：条目物理路径原样留着，取件仍按它从 zip 里抽字节
    let root_prefix = match &mods_prefix {
        ModsLayout::Prefix(p) => p[..p.len() - "mods/".len()].to_string(),
        ModsLayout::Root => String::new(),
    };

    let mut mod_files = Vec::new();
    let mut extra_files = Vec::new();
    for (name, size) in &entries {
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
            });
        } else if let Some(i) = lower.find('/') {
            // 非 mods 的目录文件全部收录（kubejs/地图/材质包等），供「客户端保留目录」卡勾选；
            // 启动器/运行时的重量级目录剔除，避免解析出上万条目
            let top = &lower[..i];
            if !top.starts_with("mods") && !ZIP_SKIP_TOP_DIRS.contains(&top) {
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
                });
            }
        }
    }

    if mod_files.is_empty() {
        return Err("mods 目录中没有任何 .jar 模组文件".into());
    }

    let mc_version = guess_mc_version(path, &entries).unwrap_or_else(|| "1.20.1".into());
    let loader = guess_loader(&entries, path);

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
        loader_version: None,
        root_prefix,
    })
}

/// 找到直接存放 jar 的 mods 目录（兼容 "mods/" 或 "packname/mods/"）；
/// Root 表示 jar 直接散落在 zip 根目录的裸包
enum ModsLayout {
    Prefix(String),
    Root,
}

fn detect_mods_prefix(entries: &[(String, u64)]) -> Option<ModsLayout> {
    for (name, _) in entries {
        let lower = name.replace('\\', "/").to_lowercase();
        if !lower.ends_with(".jar") {
            continue;
        }
        if let Some(i) = lower.find("mods/") {
            let rest = &lower[i + 5..];
            if !rest.contains('/') {
                return Some(ModsLayout::Prefix(name[..i + 5].to_string()));
            }
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

fn first_version_like(s: &str) -> Option<String> {
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

fn guess_loader(entries: &[(String, u64)], path: &Path) -> LoaderKind {
    let has = |kw: &str| entries.iter().any(|(n, _)| n.to_lowercase().contains(kw));
    if has("neoforge") {
        LoaderKind::NeoForge
    } else if has("forge") {
        LoaderKind::Forge
    } else if has("fabric") {
        LoaderKind::Fabric
    } else {
        let file = path
            .file_name()
            .map(|s| s.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if file.contains("neoforge") {
            LoaderKind::NeoForge
        } else if file.contains("forge") {
            LoaderKind::Forge
        } else {
            LoaderKind::Fabric
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// 构造一个最小 mrpack：MC 版本只在 dependencies 里，
    /// 含 1 个下载条目 + 1 个包内自带（local）模组 + 1 个自带配置文件
    fn write_fake_mrpack() -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "sideshift-parser-{}.mrpack",
            uuid::Uuid::new_v4()
        ));
        let mut w = zip::ZipWriter::new(File::create(&path).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        w.start_file(MRPACK_ENTRY, opts).unwrap();
        write!(
            w,
            r#"{{
  "game": "minecraft",
  "dependencies": {{ "minecraft": "1.21.1", "fabric-loader": "0.16.9" }},
  "files": [
    {{ "path": "mods/dl.jar", "hashes": {{}}, "downloads": ["https://x/dl.jar"] }},
    {{ "path": "mods/local.jar", "hashes": {{}}, "downloads": [], "fileSize": 4 }},
    {{ "path": "config/x.toml", "hashes": {{}}, "downloads": [], "fileSize": 2 }}
  ]
}}"#
        )
        .unwrap();
        w.start_file("mods/local.jar", opts).unwrap();
        w.write_all(b"junk").unwrap();
        w.start_file("config/x.toml", opts).unwrap();
        w.write_all(b"x=1").unwrap();
        // 未声明文件：手动拖进 zip 的 kubejs 脚本（index.files 里没有这个条目）
        w.start_file("kubejs/client_scripts/demo.js", opts).unwrap();
        w.write_all(b"console.info('hi')").unwrap();
        w.finish().unwrap();
        path
    }

    #[test]
    fn mrpack_mc_version_from_dependencies_not_game() {
        let path = write_fake_mrpack();
        let parsed = parse(&path);
        let _ = std::fs::remove_file(&path);
        assert!(parsed.manifest.parsed, "{:?}", parsed.manifest.error);
        assert_eq!(parsed.manifest.mc_version, "1.21.1");
        assert_eq!(parsed.loader_version.as_deref(), Some("0.16.9"));
    }

    #[test]
    fn mrpack_without_env_declares_no_side_flag() {
        // 上面的 fixture 全程没写 env 段：真实包里这是常态。
        // 若这里补成 Optional，下游会误当「作者已声明」，证据阶梯后几层全部失效。
        let path = write_fake_mrpack();
        let parsed = parse(&path);
        let _ = std::fs::remove_file(&path);
        for f in &parsed.mod_files {
            assert!(
                f.env_client.is_none() && f.env_server.is_none(),
                "{} 无 env 声明时不应有端标志",
                f.path
            );
        }
    }

    #[test]
    fn mrpack_counts_local_and_downloaded_mods() {
        let path = write_fake_mrpack();
        let parsed = parse(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(parsed.manifest.mod_count, 2); // dl + local 都算
        assert_eq!(parsed.mod_files.len(), 2);
        // 自带文件：url 为空（流水线据此走 Fetch::ZipEntry 从源包抽取）
        assert!(
            parsed
                .mod_files
                .iter()
                .find(|f| f.path == "mods/local.jar")
                .unwrap()
                .url
                .is_empty()
        );
        assert!(!parsed
            .mod_files
            .iter()
            .find(|f| f.path == "mods/dl.jar")
            .unwrap()
            .url
            .is_empty());
        // 非 mods 的自带文件归入 extra_files；另含 1 个未声明的 kubejs 物理文件
        assert_eq!(parsed.extra_files.len(), 2);
        assert_eq!(parsed.extra_files[0].path, "config/x.toml");
        let undeclared = parsed
            .extra_files
            .iter()
            .find(|f| f.path == "kubejs/client_scripts/demo.js")
            .expect("未声明的 zip 文件应被补收");
        assert!(undeclared.url.is_empty()); // 构建时走 ZipEntry 直接从源包抽取
    }

    /// 保留范围硬闸：判据是「路径里任一段」，所以 `config/mods` 也拦——剪层落位之后它就是包根 `mods/`。
    /// 段全等，`mods_x` 不是 mods（裸 zip 收录侧的 `starts_with("mods")` 是同族问题，未收进这里）
    #[test]
    fn keep_gate_covers_top_dirs_on_both_sides() {
        assert!(keep_denied("mods"));
        assert!(keep_denied("MODS/Some.jar"));
        assert!(keep_denied("resourcepacks/x.zip"));
        assert!(keep_denied(logical_rel("overrides/ResourcePacks/x.zip")));
        assert!(!keep_denied("config"));
        assert!(!keep_denied("config/jei/jei.ini"));
        assert!(keep_denied("config/mods"));
        assert!(keep_denied("MyPack/mods/fabric/a.jar"));
        assert!(!keep_denied("mods_backup/a.cfg"));
        assert_eq!(base_name("kubejs/client_scripts"), "client_scripts");
        assert_eq!(base_name("options.txt"), "options.txt");
    }

    /// 勾哪一层就落哪一层：内部层级保留，祖先剪掉；顶层勾选恒等（旧勾选值不受影响）
    #[test]
    fn kept_rel_lands_the_picked_level_at_root() {
        assert_eq!(kept_rel("config", "config/jei/jei.ini"), "config/jei/jei.ini");
        assert_eq!(kept_rel("config/jei", "config/jei/jei.ini"), "jei/jei.ini");
        assert_eq!(
            kept_rel("kubejs/client_scripts", "kubejs/client_scripts/demo.js"),
            "client_scripts/demo.js"
        );
        assert_eq!(kept_rel("kubejs/startup.js", "kubejs/startup.js"), "startup.js");
        // 落位名跟条目自己的大小写，不跟小写勾选键
        assert_eq!(kept_rel("config/jei", "Config/JEI/jei.ini"), "JEI/jei.ini");
        // 勾选键比条目还深（匹配逻辑出错才会出现）⇒ 无从落位，返回空串让调用方跳过，不能落到包根
        assert_eq!(kept_rel("config/jei/jei.ini/deep", "config/jei/jei.ini"), "");
    }

    /// 裸 zip 外面套一层自定义文件夹：mods 那条腿早就按 `MyPack/mods/` 探测了，保留内容这条腿
    /// 必须剥同一层——否则勾选值与落位都带着 `MyPack/`，而服务端按实例根读 `config/`
    #[test]
    fn bare_zip_wrapper_folder_leaves_logical_paths() {
        let path = std::env::temp_dir().join(format!(
            "sideshift-parser-{}.zip",
            uuid::Uuid::new_v4()
        ));
        let mut w = zip::ZipWriter::new(File::create(&path).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        w.start_file("MyPack/mods/a.jar", opts).unwrap();
        w.write_all(b"PK\x03\x04").unwrap();
        // mods 下的非 jar 会落进 extra_files（mods 判据只看 jar），靠段名闸拦住
        w.start_file("MyPack/mods/README.md", opts).unwrap();
        w.write_all(b"r").unwrap();
        w.start_file("MyPack/config/jei/jei.ini", opts).unwrap();
        w.write_all(b"x").unwrap();
        w.start_file("MyPack/options.txt", opts).unwrap();
        w.write_all(b"y").unwrap();
        w.finish().unwrap();

        let parsed = parse(&path);
        assert_eq!(parsed.root_prefix, "MyPack/");
        assert_eq!(parsed.extra_files.len(), 3);
        // 物理路径留着（取件按它从 zip 里抽字节），逻辑路径才是交付包里的位置
        assert_eq!(parsed.extra_files[0].path, "MyPack/config/jei/jei.ini");
        assert_eq!(parsed.logical_rel("MyPack/config/jei/jei.ini"), "config/jei/jei.ini");
        assert_eq!(parsed.logical_rel("MyPack/options.txt"), "options.txt");
        // 前缀是从某一条 jar 探出来的，别的条目大小写不同也要剥
        assert_eq!(parsed.logical_rel("mypack/Config/a.toml"), "Config/a.toml");
        // 不在这层文件夹下的路径不动它
        assert_eq!(parsed.logical_rel("other/x.cfg"), "other/x.cfg");
        // mods 下的非 jar 会落进 extra_files（jar 才归模组那条腿）：剥完前缀正好撞上段名闸，
        // 不剥的话它叫 `mypack/mods/…`——段名闸照样拦得住，两道保险叠着，落位剪层也不给后门
        let mut denied: Vec<String> = Vec::new();
        for f in &parsed.extra_files {
            if keep_denied(parsed.logical_rel(&f.path)) {
                denied.push(f.path.clone());
            }
        }
        assert_eq!(denied, vec!["MyPack/mods/README.md".to_string()]);
    }
}
