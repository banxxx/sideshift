//! 整合包解析：.mrpack 精确解析（modrinth.index.json）；裸 .zip 启发式扫描 mods/ 目录。
//!
//! mrpack 规范口径：顶层 `game` 恒为游戏 ID（"minecraft"），MC 版本在 `dependencies.minecraft`；
//! `files[]` 中 downloads 为空的条目是"包内自带"（local）文件，同样计入模组。

use std::collections::BTreeMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use serde::Deserialize;

use crate::models::{LoaderKind, PackManifest};

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
    /// env.server != unsupported
    pub server_required: bool,
    /// 条目是否声明了 env 段（声明则按 env 精确判定，否则走名称启发式）
    pub env_declared: bool,
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
}

pub const MRPACK_ENTRY: &str = "modrinth.index.json";

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
}

#[derive(Deserialize)]
struct RawEnv {
    server: Option<String>,
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

    let mut mod_files = Vec::new();
    let mut extra_files = Vec::new();
    for f in &index.files {
        let file_name = Path::new(&f.path)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| f.path.clone());
        let server_side = f.env.as_ref().and_then(|e| e.server.as_deref());
        // downloads 为空 = 包内自带（local）文件：同样计入模组与方案，
        // 构建时由流水线经 Fetch::ZipEntry 直接从源包抽取，不联网
        let pf = PackFile {
            path: f.path.clone(),
            file_name,
            url: f.downloads.first().cloned().unwrap_or_default(),
            sha1: f.hashes.get("sha1").cloned(),
            server_required: server_side != Some("unsupported"),
            env_declared: f.env.as_ref().and_then(|e| e.server.clone()).is_some(),
        };
        if pf.path.starts_with("mods/") && pf.file_name.ends_with(".jar") {
            mod_files.push(pf);
        } else {
            extra_files.push(pf);
        }
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
    })
}

/* ---------------- 裸 zip（启发式） ---------------- */

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

    let mut mod_files = Vec::new();
    let mut extra_files = Vec::new();
    for (name, _) in &entries {
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
                server_required: true,
                env_declared: false,
            });
        } else if lower.ends_with(".toml") || lower.ends_with(".json") || lower.ends_with(".cfg")
        {
            extra_files.push(PackFile {
                path: name.clone(),
                file_name,
                url: String::new(),
                sha1: None,
                server_required: true,
                env_declared: false,
            });
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
        source_path: None,
    };
    Ok(ParsedPack {
        manifest,
        mod_files,
        extra_files,
        loader_version: None,
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
        // 非 mods 的自带文件归入 extra_files
        assert_eq!(parsed.extra_files.len(), 1);
        assert_eq!(parsed.extra_files[0].path, "config/x.toml");
    }
}
