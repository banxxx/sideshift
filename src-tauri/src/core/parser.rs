//! 整合包解析：**按内容分派**（zip 里有 `modrinth.index.json` 就走 mrpack 精确解析，
//! 认不出清单才走裸包启发式扫 mods/），扩展名只决定「探不到清单时是报错还是降级」。
//!
//! mrpack 规范口径：顶层 `game` 恒为游戏 ID（"minecraft"），MC 版本在 `dependencies.minecraft`；
//! `files[]` 条目应全部物理内嵌于 zip——以 zip 条目实测判定 `in_pack`，缺字节的残缺条目才回落 URL 下载。

use std::collections::BTreeMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use serde::Deserialize;

use crate::models::{CfLink, LoaderKind, PackManifest, SideFlag};

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
    /// CF 清单的一条编号：`files[]` 只给 `projectID`/`fileID`，**字节、名字、大小、校验值都不在包里**。
    /// 有这一枚就意味着这一行得联网向 CF 要（元数据 + 临时直链），离线做不了
    pub cf: Option<CfRef>,
}

/// CurseForge 的构建坐标（`/v1/mods/{modId}/files/{fileId}`）。id 存字符串：CF 给数字，
/// 但取链、方案行 id、前端那批 CF 行都按字符串走（同一套形状，别引入第二套）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CfRef {
    pub mod_id: String,
    pub file_id: String,
    /// `files[].required`：清单说这枚模组是必需还是可选。字段缺失按**必需**处理
    /// （民间改过的清单真会漏掉它，漏掉就当可选等于替用户决定"少了也能跑"）
    pub required: bool,
    /// 取链许可态：解析层恒 `Unknown`（没探过），由 `cfpack::ensure` 从索引贴回来。
    /// 挂在这一枚上而不是 `PackFile` 的新字段：它属于"这对编号能不能拿到字节"，
    /// 与 `mod_id`/`file_id` 同生命周期，加字段就不用改六处 `PackFile` 构造点
    pub link: CfLink,
}

#[derive(Debug, Clone)]
pub struct ParsedPack {
    pub manifest: PackManifest,
    /// mods/ 目录下的模组 jar（含 URL/env 信息，供 detector/downloader 消费）
    pub mod_files: Vec<PackFile>,
    /// 非 mods 资源文件（config 等）
    pub extra_files: Vec<PackFile>,
    /// mrpack 的 dependencies 段（minecraft/fabric-loader 版本）；裸 zip 时来自 mods 里那枚加载器 jar
    /// 的文件名，认不出就是 None（前端落到候选列表的推荐项上）
    pub loader_version: Option<String>,
    /// 清单/jar 外面的「包根外层文件夹」（`MyPack/mods/` 探出来的 `MyPack/`）。
    /// 逻辑路径换算要先剥它——不然勾选值、保留树、落位、预估全都会多出这一层，
    /// 而服务端按实例根读 `config/`，带着一层自定义目录的产物根本读不到。
    /// 裸 zip 与「改名成 .zip 且套了一层壳的 mrpack」都走这一条；CF 那层 `overrides/` 由 logical_rel 剥
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

/// CurseForge 导出格式（MCBBS 那支长得一模一样，只多一个 `addons[]`）的清单名
pub const CF_ENTRY: &str = "manifest.json";

/// 探不到清单时的实话（扩展名写着 .mrpack 也用这一句，不降级去跑启发式）
const NO_INDEX: &str = "缺少 modrinth.index.json，不是有效的 Modrinth 整合包";

/// 启发式那条腿探到 mods 目录时的报错
const NO_MODS_DIR: &str = "未找到 mods 目录，不是可识别的整合包（推荐直接使用 .mrpack）";

/// CF 清单**既没声明条目、包里也没有 jar**时的实话。
/// 「清单只给编号、字节不在包里」那一档不再报错——那是能转换的（编号拿去向 CF 补取），
/// 只有连编号都没有时才真的没内容可转
const CF_NOTHING: &str = "这份 CurseForge 清单没有声明任何模组，包里也没有 jar 字节";

/// 格式壳目录：mrpack/CF 清单**外面不该再套**它们，套上了说明这一层是内容不是包根
/// （Prism 源码明确警告 `manifest.json` 会出现在 `overrides/` 里面 ⇒ 全条目子串查找会把整包认错位）
const FORMAT_SHELLS: &[&str] = &[
    "overrides",
    "override",
    "client-overrides",
    "server-overrides",
];

/// zip 条目名（不含目录条目）。分派要先看内容，故与真正的解析各开一次包——
/// 读的是中央目录，毫秒级，比把解析函数改成收 archive 借用再到处传要小得多
fn zip_names(path: &Path) -> Result<Vec<String>, String> {
    let zip = File::open(path).map_err(|e| format!("无法打开文件：{e}"))?;
    let mut archive = zip::ZipArchive::new(zip).map_err(|e| format!("zip 结构损坏：{e}"))?;
    let mut names = Vec::new();
    for i in 0..archive.len() {
        let f = archive.by_index(i).map_err(|e| e.to_string())?;
        if !f.is_dir() {
            names.push(f.name().to_string());
        }
    }
    Ok(names)
}

/// 内容优先定位清单文件 ⇒（清单条目名，包根外层文件夹）。
///
/// 只认两层：**包根**（`modrinth.index.json`）与**单段外层文件夹**（`MyPack/modrinth.index.json`，
/// 民间压缩包常见的一层套壳）。再深就不是包根了；也不递归剥（PCL2 与 Prism 同口径：
/// 都只按命中文件的父一级定 `ArchiveBaseFolder`，剥完不再探第二层）。
/// 外层文件夹叫格式壳那几个名字的直接跳过——那是内容层，剥它等于把清单从内容里挖出来当包根。
fn find_manifest(names: &[String], wanted: &str) -> Option<(String, String)> {
    for name in names {
        let norm = name.replace('\\', "/");
        let segs: Vec<&str> = norm.split('/').collect();
        let (idx, prefix) = match segs.len() {
            1 if segs[0].eq_ignore_ascii_case(wanted) => (norm.clone(), String::new()),
            2 if segs[1].eq_ignore_ascii_case(wanted)
                && !FORMAT_SHELLS.contains(&segs[0].to_lowercase().as_str()) =>
            {
                // 前缀跟着条目原样大小写走（`strip_root_prefix` 自己大小写不敏感，这里只为落盘名一致）
                (norm.clone(), format!("{}/", segs[0]))
            }
            _ => continue,
        };
        return Some((idx, prefix));
    }
    None
}

/// 三家清单的先后：Modrinth 的 `modrinth.index.json` → CurseForge/MCBBS 的 `manifest.json` → 裸包启发式。
/// 扩展名写的是 `.mrpack` 却两份清单都探不到时**不降级**跑启发式（那会把「这不是 Modrinth 包」
/// 这句实话换成一套猜出来的方案），仍按老口径报错
fn parse_by_content(path: &Path, ext: &str) -> Result<ParsedPack, String> {
    let names = zip_names(path)?;
    if let Some((idx, prefix)) = find_manifest(&names, MRPACK_ENTRY) {
        return parse_mrpack(path, &idx, &prefix);
    }
    if let Some((idx, _)) = find_manifest(&names, CF_ENTRY) {
        // `manifest.json` 这名字太通用：内容不像 CF 那份形状就当没有它，不能让一个不相干的
        // manifest 把整包从启发式那条路上带走
        if let Some(facts) = cf_declared_facts(path, &idx)? {
            return scan_zip_entries(path, &facts, CF_NOTHING);
        }
    }
    if ext == "mrpack" {
        return Err(NO_INDEX.to_string());
    }
    scan_zip_entries(path, &Declared::default(), NO_MODS_DIR)
}

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

/// 解析入口：**先按内容分派，扩展名只决定探不到清单时的退路**。
/// 任何失败都返回 parsed:false 的 manifest（不 panic）
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
            // mrpack 与 zip 走同一条内容探测：民间打包工具（以及「把 mrpack 改名成 zip」这一常见做法）
            // 给出的扩展名并不可信，认扩展名等于把包里的清单扔掉不用
            "mrpack" | "zip" => parse_by_content(path, &ext),
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

/// mrpack 精确解析。`index_entry` 是清单在 zip 里的**物理条目名**，`root_prefix` 是它外面那层
/// 自定义文件夹（改名成 `.zip` 的 mrpack 套了一层壳时非空）。
/// 除此之外与老写法逐条一致：清单行仍按**声明路径**（相对实例根，不带壳）判 in_pack，
/// 物理缺失就照旧回落 URL。
fn parse_mrpack(
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

/* ---------------- CurseForge / MCBBS 清单 ---------------- */

/// CF 那份 `manifest.json` 里本轮真正用到的两段。
/// **只声明编号、不内嵌 jar**：`files[]` 是 `{projectID,fileID,required}`，字节得联网取
/// （`overrides` 字段能改写格式壳的名字，但那条腿不需要读它——物理扫描按 `…/mods/` 自己定包根，
/// 名字改成什么都跟着走，写死反而多一处会过期的假设）
#[derive(Deserialize, Default)]
struct CfManifest {
    #[serde(default)]
    minecraft: CfMinecraft,
    #[serde(default)]
    files: Vec<CfFile>,
}

#[derive(Deserialize, Default)]
struct CfMinecraft {
    #[serde(default)]
    version: String,
    #[serde(default, rename = "modLoaders")]
    mod_loaders: Vec<CfModLoader>,
}

#[derive(Deserialize)]
struct CfModLoader {
    #[serde(default)]
    id: String,
}

/// `files[]` 的一条：CF 官方导出的包**只给这一对编号**，jar 字节、文件名、大小、校验值都不在包里
#[derive(Deserialize)]
struct CfFile {
    #[serde(default, rename = "projectID")]
    project_id: serde_json::Value,
    #[serde(default, rename = "fileID")]
    file_id: serde_json::Value,
    /// 缺字段按**必需**处理：把漏写当成可选，等于替用户决定「这枚少了也能跑」
    #[serde(default = "cf_required_default", rename = "required")]
    required: bool,
}

fn cf_required_default() -> bool {
    true
}

/// CF 那对 id 有的是数字、个别老包给字符串，两种都得吃（与 downloader 侧 `ident()` 同一口径）
fn cf_id(v: &serde_json::Value) -> String {
    v.as_u64()
        .map(|i| i.to_string())
        .or_else(|| v.as_str().map(|s| s.trim().to_string()))
        .unwrap_or_default()
}

/// 清单能替启发式说话的那几档（`None` = 这份清单没说，照旧走猜的那条路）
#[derive(Default, Clone)]
struct Declared {
    mc_version: Option<String>,
    loader: Option<LoaderKind>,
    loader_version: Option<String>,
    /// 清单声明的模组编号（去重后）。**空 = 这份清单没列模组**，与「列了但字节不在包里」是两件事
    files: Vec<CfRef>,
    /// 认下来的那份清单自己在包里的物理条目名。它是格式自带的文件、不是「包里的内容」，
    /// 不带这一档就会把 `manifest.json` 摆进保留清单里让人勾（勾上等于把编号表拷进服务端包）
    index_entry: Option<String>,
}

/// `modLoaders[].id` 的形状是 `<家族>-<版本>`：`forge-47.2.20`、`neoforge-21.1.77`、`fabric-0.15.3`。
/// 家族段**全等**比（`neoforge` 不能被子串 `forge` 抢走），认不出的家族（optifine、rift 这些）
/// 直接跳过——`LoaderKind` 只有三档，硬塞一档等于给下载器一个不存在的坐标。
/// 版本段照 Forge 候选列表的口径给**纯构建号**（CF 的 id 里本来就不带 MC 前缀）
fn cf_loader(id: &str) -> Option<(LoaderKind, Option<String>)> {
    let (kind, ver) = id.split_once('-')?;
    let ver = Some(ver.trim().to_string()).filter(|s| !s.is_empty());
    match kind.trim().to_lowercase().as_str() {
        "fabric" | "fabric-loader" => Some((LoaderKind::Fabric, ver)),
        "neoforge" => Some((LoaderKind::NeoForge, ver)),
        "forge" => Some((LoaderKind::Forge, ver)),
        _ => None,
    }
}

/// 读出 CF 清单里那份声明；**内容不像 CF 就返回 None**（`manifest.json` 这名字太通用，
/// 一个不相干的 manifest 不该把整包从启发式那条路上带走），读文件或解 JSON 失败也一律 None：
/// 启发式那条腿还活着，没必要因为一份可选声明把整包判死
fn cf_declared_facts(path: &Path, entry: &str) -> Result<Option<Declared>, String> {
    let zip = File::open(path).map_err(|e| format!("无法打开文件：{e}"))?;
    let mut archive = zip::ZipArchive::new(zip).map_err(|e| format!("zip 结构损坏：{e}"))?;
    let Ok(mut f) = archive.by_name(entry) else {
        return Ok(None);
    };
    let mut buf = Vec::new();
    if f.read_to_end(&mut buf).is_err() {
        return Ok(None);
    }
    let Ok(m) = serde_json::from_slice::<CfManifest>(&buf) else {
        return Ok(None);
    };
    let mc_version = Some(m.minecraft.version.trim().to_string()).filter(|s| !s.is_empty());
    // 多个 modLoaders 条目时取**第一个认得出的家族**（CF 的导出顺序把主加载器放前面）
    let loader = m
        .minecraft
        .mod_loaders
        .iter()
        .find_map(|l| cf_loader(&l.id));
    // `files[]` 去重：同一对编号重复列过（手工改过的清单真有这种），一行编号 = 方案一行，
    // 留重复等于同一个 jar 下两遍
    let mut files: Vec<CfRef> = Vec::new();
    for f in &m.files {
        let mod_id = cf_id(&f.project_id);
        let file_id = cf_id(&f.file_id);
        if mod_id.is_empty() || file_id.is_empty() {
            continue;
        }
        let r = CfRef { mod_id, file_id, required: f.required, link: CfLink::Unknown };
        // 去重按**坐标那一对 id**，不按整个结构体：`required` 在同一对编号的两条声明里
        // 给得不一样是真有的事（手改过清单），按整结构体比就会留下两行、同一个 jar 下两遍
        if !files.iter().any(|x| x.mod_id == r.mod_id && x.file_id == r.file_id) {
            files.push(r);
        }
    }
    // 三段全空才叫「内容不像 CF」：只列模组、不写 minecraft 的清单仍然是 CF 形状的证据；
    // 同名太通用，三段全空的那份 manifest.json 不该把整包从启发式那条路上带走
    if mc_version.is_none() && loader.is_none() && files.is_empty() {
        return Ok(None);
    }
    Ok(Some(Declared {
        mc_version,
        loader: loader.as_ref().map(|(k, _)| *k),
        loader_version: loader.and_then(|(_, v)| v),
        files,
        index_entry: Some(entry.to_string()),
    }))
}

/// 按声明的编号造方案行。**这一档没有字节**：路径/名字只是锚点（`split_mod_file` 从
/// `{mod_id}-{file_id}` 切出 `id`/`version`，`detector` 的 `src_path` 也按它精确回指），
/// 真名、大小、sha1、直链由 `core::cfpack` 联网补；补不到就停在「未就位」，不假装在包里
fn cf_rows(files: &[CfRef]) -> Vec<PackFile> {
    files
        .iter()
        .map(|r| {
            let file_name = format!("{}-{}.jar", r.mod_id, r.file_id);
            PackFile {
                path: format!("mods/{file_name}"),
                file_name,
                url: String::new(),
                sha1: None,
                size_bytes: 0,
                in_pack: false,
                env_server: None,
                env_client: None,
                depends: Vec::new(),
                cf: Some(r.clone()),
            }
        })
        .collect()
}

/* ---------------- 按物理条目扫一遍（裸包启发式 / CF 清单+自带 jar） ---------------- */

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

/// 逐条扫 zip 里的物理文件：`mods` 段定包根，jar 归模组、其余归「保留内容」。
///
/// 这条腿服务两种包：什么清单都没有的**裸 zip**（`declared` 全 None ⇒ 版本与加载器只能猜），
/// 以及 **CF/MCBBS 那种「清单只说家族与版本、jar 仍躺在 overrides/mods 里」的民间包**
/// （`declared` 有值 ⇒ 声明压过猜测）。
/// `no_jar_msg` 是「一条 jar 都没扫出来」时的实话：裸包和 CF 包的病因不同，不能共用一句
fn scan_zip_entries(path: &Path, declared: &Declared, no_jar_msg: &str) -> Result<ParsedPack, String> {
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
enum ModsLayout {
    Prefix(String),
    Root,
}

/// mods 目录的判据：**「文件名所在那一段」全等于 `mods`**（大小写不敏感），不是路径里出现过 `mods/`。
/// 子串那条写法会把 `somemods/`、`SomePack_mods/` 这类目录当成模组目录，整包的模组与保留内容
/// 从此按一个不存在的目录分家——和 `keep_denied` 早就是段全等，两侧口径在这里对齐。
fn detect_mods_prefix(entries: &[(String, u64)]) -> Option<ModsLayout> {
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

/// 加载器 jar 的文件名 ⇒（加载器，那一档版本号）。
/// 认的是官方构建产物的名字形状：`fabric-loader-0.16.9.jar`、
/// `neoforge-21.1.77-universal.jar`、`forge-1.20.1-47.2.20-universal.jar`。
/// 认不出版本一律给 None（下面那档自动选择会取推荐项），**不给一个像是版本的假串**。
fn loader_from_jar_name(name: &str) -> Option<(LoaderKind, Option<String>)> {
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
    /// 段全等，`mods_x` 不是 mods（裸 zip 的 mods 定位与收录侧两条腿已按同一口径收口，见
    /// `mods_dir_matches_the_whole_segment`）
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

    /// 猜不到 MC 版本 ⇒ 空串。旧的 1.20.1 兜底把「没线索」演成「这一档就是 1.20.1」，
    /// 于是 Java 需求线（17）、Loader 候选、模组反查全按一个凭空造的版本跑；空值才是实话，
    /// 界面据此显示「未识别」、开始转换那道闸据此停住。
    #[test]
    fn bare_zip_without_any_version_hint_parses_as_empty_mc_version() {
        let path = std::env::temp_dir().join(format!(
            "sideshift-parser-{}.zip",
            uuid::Uuid::new_v4()
        ));
        let mut w = zip::ZipWriter::new(File::create(&path).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        w.start_file("mods/a.jar", opts).unwrap();
        w.write_all(b"PK\x03\x04").unwrap();
        w.start_file("config/x.cfg", opts).unwrap();
        w.write_all(b"y").unwrap();
        w.finish().unwrap();

        let parsed = parse(&path);
        let _ = std::fs::remove_file(&path);
        assert!(parsed.manifest.parsed, "{:?}", parsed.manifest.error);
        assert_eq!(parsed.manifest.mc_version, "");
        // 空版本不该被下游当成某一档：兜底表对认不出的串给 17，那是「不知道要哪档」而不是「这就是 17」
        assert_eq!(crate::core::java::required_for_mc(""), "17");
    }

    /// 有线索时照旧猜：条目名里带 `minecraft`/`version.json`，或包文件名本身带版本号
    #[test]
    fn bare_zip_still_reads_version_from_entry_names_and_file_name() {
        assert_eq!(
            first_version_like("minecraft-version-1.21.1.json"),
            Some("1.21.1".to_string())
        );
        // 裸包名（快照那种没有 `1.` 前缀的年份号）认不出来 ⇒ 空，而不是硬凑
        assert_eq!(first_version_like("24w14a"), None);
    }

    /// mods 定位改段全等：旧写法拿子串找 `mods/`，`somemods/a.jar` 会被当成模组目录，
    /// 整包的模组与保留内容从此按一个不存在的目录分家（jar 全数进模组清单、真 `mods/` 反而没人认）
    #[test]
    fn mods_dir_matches_the_whole_segment() {
        let e = |names: &[&str]| -> Vec<(String, u64)> {
            names.iter().map(|n| (n.to_string(), 1u64)).collect()
        };
        let prefix = |names: &[&str]| -> Option<String> {
            match detect_mods_prefix(&e(names)) {
                Some(ModsLayout::Prefix(p)) => Some(p),
                Some(ModsLayout::Root) => Some("<root>".into()),
                None => None,
            }
        };
        assert_eq!(prefix(&["mods/a.jar"]).as_deref(), Some("mods/"));
        // 大小写不敏感，前缀按原样条目名切（`in_mods` 拿它跟原始路径比）
        assert_eq!(prefix(&["MyPack/MODS/a.jar"]).as_deref(), Some("MyPack/MODS/"));
        assert_eq!(prefix(&["a.jar"]).as_deref(), Some("<root>"));
        // 不是 mods 的那些：子串命中、名字相近、jar 不在 mods 那一层里
        assert_eq!(prefix(&["somemods/a.jar"]), None);
        assert_eq!(prefix(&["mods_backup/a.jar", "config/x.cfg"]), None);
        assert_eq!(prefix(&["mods/nested/a.jar"]), None);
        // 两样都在时，位置由真 `mods/` 那枚定，相近名不抢位
        assert_eq!(prefix(&["somemods/a.jar", "mods/b.jar"]).as_deref(), Some("mods/"));
    }

    /// 段全等同时收口了「收录侧」那条 `starts_with("mods")`：`mods_backup/` 不再被当作模组目录丢掉，
    /// 它是「用户要不要留下的内容」；而 `mods/` 本身照旧不进保留清单（那里有 `keep_denied` 第二道闸）
    #[test]
    fn mods_like_dirs_are_not_the_mods_dir() {
        let path = std::env::temp_dir().join(format!(
            "sideshift-parser-{}.zip",
            uuid::Uuid::new_v4()
        ));
        let mut w = zip::ZipWriter::new(File::create(&path).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        w.start_file("mods/a.jar", opts).unwrap();
        w.write_all(b"PK\x03\x04").unwrap();
        w.start_file("mods/README.md", opts).unwrap();
        w.write_all(b"r").unwrap();
        w.start_file("mods_backup/x.cfg", opts).unwrap();
        w.write_all(b"c").unwrap();
        w.start_file("somemods/b.jar", opts).unwrap();
        w.write_all(b"PK\x03\x04").unwrap();
        w.finish().unwrap();

        let parsed = parse(&path);
        let _ = std::fs::remove_file(&path);
        assert!(parsed.manifest.parsed, "{:?}", parsed.manifest.error);
        // 模组清单只认 `mods/` 那一层
        assert_eq!(parsed.mod_files.len(), 1);
        assert_eq!(parsed.mod_files[0].path, "mods/a.jar");
        let extras: Vec<&str> = parsed.extra_files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(extras, vec!["mods_backup/x.cfg", "somemods/b.jar"]);
    }

    /// 造一个「内容层级就是 mrpack 那一套、扩展名却是 .zip」的包（民间打包工具与手动改名都这样）。
    /// `index_at` = 清单在包里的条目名；其余内容按清单所在那一层摆（套壳包整棵都在壳里，正经包在根）
    fn write_mrpack_shape_zip(index_at: &str) -> std::path::PathBuf {
        let shell = index_at
            .rsplit_once('/')
            .map(|(d, _)| format!("{d}/"))
            .unwrap_or_default();
        let path = std::env::temp_dir().join(format!(
            "sideshift-parser-{}.zip",
            uuid::Uuid::new_v4()
        ));
        let mut w = zip::ZipWriter::new(File::create(&path).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        w.start_file(index_at, opts).unwrap();
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
        // 声明过的条目：物理名 = 外层壳 + 声明路径
        w.start_file(format!("{shell}mods/local.jar"), opts).unwrap();
        w.write_all(b"junk").unwrap();
        w.start_file(format!("{shell}config/x.toml"), opts).unwrap();
        w.write_all(b"x=1").unwrap();
        // 没写进清单的物理文件（手动塞的脚本）
        w.start_file(format!("{shell}kubejs/client_scripts/demo.js"), opts)
            .unwrap();
        w.write_all(b"console.info('hi')").unwrap();
        w.finish().unwrap();
        path
    }

    /// 内容优先分派：`.zip` 里有 `modrinth.index.json` 就走 mrpack 那条腿。
    /// 这条是本轮的主案——以前按扩展名分派，改名包掉进启发式，版本/加载器/清单字段全丢
    #[test]
    fn zip_containing_mrpack_index_parses_as_mrpack() {
        let path = write_mrpack_shape_zip(MRPACK_ENTRY);
        let parsed = parse(&path);
        let _ = std::fs::remove_file(&path);
        assert!(parsed.manifest.parsed, "{:?}", parsed.manifest.error);
        assert_eq!(parsed.manifest.mc_version, "1.21.1");
        assert_eq!(parsed.loader_version.as_deref(), Some("0.16.9"));
        assert_eq!(parsed.manifest.loader, LoaderKind::Fabric);
        // 声明的两条 jar 都算模组（`dl` 走 URL、`local` 走包内字节），不再靠文件名猜
        assert_eq!(parsed.mod_files.len(), 2);
        assert_eq!(parsed.root_prefix, "");
        // 清单自己不算保留内容；声明的 config + 未声明的 kubejs 各一条
        let extras: Vec<&str> = parsed.extra_files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(extras, vec!["config/x.toml", "kubejs/client_scripts/demo.js"]);
    }

    /// 套了一层外层文件夹的改名包：清单照读，物理名带壳（取件按它抽字节），逻辑路径不带壳
    #[test]
    fn wrapped_mrpack_zip_strips_the_outer_folder() {
        let path = write_mrpack_shape_zip("MyPack/modrinth.index.json");
        let parsed = parse(&path);
        let _ = std::fs::remove_file(&path);
        assert!(parsed.manifest.parsed, "{:?}", parsed.manifest.error);
        assert_eq!(parsed.manifest.mc_version, "1.21.1");
        assert_eq!(parsed.root_prefix, "MyPack/");
        // 带壳的物理名要能对上号：`in_pack` 判错会让包内字节白躺着、转头去联网下载
        let local = parsed
            .mod_files
            .iter()
            .find(|f| f.file_name == "local.jar")
            .expect("声明的包内模组应在清单里");
        assert_eq!(local.path, "MyPack/mods/local.jar");
        assert!(local.in_pack);
        assert_eq!(parsed.logical_rel("MyPack/mods/local.jar"), "mods/local.jar");
        assert_eq!(parsed.logical_rel("MyPack/config/x.toml"), "config/x.toml");
        // 未声明的那条也认得到（补收腿按剥壳后的路径判重量级目录，与不套壳时同一条口径）
        assert!(parsed
            .extra_files
            .iter()
            .any(|f| f.path == "MyPack/kubejs/client_scripts/demo.js"));
    }

    /// 清单只认包根与「单段外层文件夹」这两层：`overrides/` 里那份是内容不是根（Prism 警告的误判），
    /// 再深一层也不算
    #[test]
    fn mrpack_index_is_only_taken_at_root_or_one_wrapper_level() {
        let f = |names: &[&str]| -> Option<(String, String)> {
            let v: Vec<String> = names.iter().map(|s| s.to_string()).collect();
            find_manifest(&v, MRPACK_ENTRY).map(|(i, p)| (i.to_string(), p))
        };
        assert_eq!(
            f(&["modrinth.index.json"]),
            Some(("modrinth.index.json".into(), String::new()))
        );
        assert_eq!(
            f(&["MyPack/modrinth.index.json"]),
            Some(("MyPack/modrinth.index.json".into(), "MyPack/".into()))
        );
        // 条目名跟着原样大小写走（`by_name` 要按物理名精确命中），判据本身不敏感
        assert_eq!(
            f(&["MyPack/MODRINTH.INDEX.JSON"]),
            Some(("MyPack/MODRINTH.INDEX.JSON".into(), "MyPack/".into()))
        );
        assert_eq!(f(&["overrides/modrinth.index.json"]), None);
        assert_eq!(f(&["client-overrides/modrinth.index.json"]), None);
        assert_eq!(f(&["A/B/modrinth.index.json"]), None);
        assert_eq!(f(&["mods/a.jar", "config/x.toml"]), None);
        // 壳里那份不抢位：根上有了就取根上的
        assert_eq!(
            f(&["overrides/modrinth.index.json", "modrinth.index.json"])
                .map(|(i, _)| i),
            Some("modrinth.index.json".to_string())
        );
    }

    /// 探不到清单的 `.zip` 照旧走启发式（本轮只加分派，没动那条腿的判据）：
    /// `overrides/` 里躺着清单不算数，包根也没有版本线索 ⇒ 版本空、模组按 mods 段名收
    #[test]
    fn zip_without_a_root_index_falls_back_to_heuristics() {
        let path = std::env::temp_dir().join(format!(
            "sideshift-parser-{}.zip",
            uuid::Uuid::new_v4()
        ));
        let mut w = zip::ZipWriter::new(File::create(&path).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        w.start_file("overrides/modrinth.index.json", opts)
            .unwrap();
        w.write_all(b"{}").unwrap();
        w.start_file("mods/fabric-loader-0.16.9.jar", opts)
            .unwrap();
        w.write_all(b"PK\x03\x04").unwrap();
        w.finish().unwrap();

        let parsed = parse(&path);
        let _ = std::fs::remove_file(&path);
        assert!(parsed.manifest.parsed, "{:?}", parsed.manifest.error);
        // 启发式那条腿读不了清单，认不出版本 ⇒ 空值（mrpack 那条腿会给 1.21.1，据此分辨走了哪条腿）
        assert_eq!(parsed.manifest.mc_version, "");
        assert_eq!(parsed.mod_files.len(), 1);
        assert_eq!(parsed.mod_files[0].path, "mods/fabric-loader-0.16.9.jar");
    }

    /// mrpack 的 jar 常常物理躺在 `overrides/mods/` 里，而 `files[]` 写的是实例根路径。
    /// 两侧对不上号 ⇒ 包内有字节却判 in_pack=false（转头联网重下），未声明那批更会被当成普通
    /// 保留内容收下、再被 `keep_denied` 在构建层吃掉，整批模组凭空消失
    #[test]
    fn mrpack_reads_jars_under_the_overrides_shell() {
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
  "dependencies": {{ "minecraft": "1.20.1", "forge": "47.2.20" }},
  "files": [
    {{ "path": "mods/shell.jar", "hashes": {{}}, "downloads": [], "fileSize": 8 }},
    {{ "path": "config/x.toml", "hashes": {{}}, "downloads": [], "fileSize": 2 }}
  ]
}}"#
        )
        .unwrap();
        w.start_file("overrides/mods/shell.jar", opts).unwrap();
        w.write_all(b"PK\x03\x04").unwrap();
        // 没写进清单的本地 jar：启动器原样装进 mods/，方案也必须收
        w.start_file("overrides/mods/handmade.jar", opts).unwrap();
        w.write_all(b"PK\x03\x04").unwrap();
        w.start_file("overrides/config/x.toml", opts).unwrap();
        w.write_all(b"x=1").unwrap();
        w.finish().unwrap();

        let parsed = parse(&path);
        let _ = std::fs::remove_file(&path);
        assert!(parsed.manifest.parsed, "{:?}", parsed.manifest.error);
        let mut mods: Vec<&str> = parsed.mod_files.iter().map(|f| f.path.as_str()).collect();
        mods.sort_unstable();
        assert_eq!(mods, vec!["overrides/mods/handmade.jar", "overrides/mods/shell.jar"]);
        // 取件按物理名从包里抽字节，所以 path 留着壳；交付位置问 logical_rel
        assert_eq!(parsed.logical_rel("overrides/mods/shell.jar"), "mods/shell.jar");
        let shell = parsed
            .mod_files
            .iter()
            .find(|f| f.file_name == "shell.jar")
            .unwrap();
        assert!(shell.in_pack, "包内有字节就该判 in_pack，不该再去联网重下");
        // 声明过的 config 只算一条（补收腿按剥壳后的路径认出它已声明）
        let extras: Vec<&str> = parsed.extra_files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(extras, vec!["overrides/config/x.toml"]);
    }

    /* ---------------- CF / MCBBS 清单这一腿 ---------------- */

    /// 官方导出的 CF 包形状：版本与加载器家族只在 `minecraft` 段里，`files[]` 只有编号
    const CF_FORGE: &str = r#"{
  "manifestType": "minecraftModpack",
  "manifestVersion": 1,
  "name": "Demo",
  "version": "1.0",
  "minecraft": { "version": "1.20.1", "modLoaders": [ { "id": "forge-47.2.20", "required": true } ] },
  "files": [ { "projectID": 1, "fileID": 2, "required": true } ],
  "overrides": "overrides"
}"#;

    /// 造一份 CF 形状的 zip：`manifest` 原样写进 `manifest_at`，其余条目按 `files` 摆
    fn write_cf_zip(manifest_at: &str, manifest: &str, files: &[&str]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "sideshift-parser-{}.zip",
            uuid::Uuid::new_v4()
        ));
        let mut w = zip::ZipWriter::new(File::create(&path).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        w.start_file(manifest_at, opts).unwrap();
        w.write_all(manifest.as_bytes()).unwrap();
        for f in files {
            w.start_file(f, opts).unwrap();
            w.write_all(b"PK\x03\x04").unwrap();
        }
        w.finish().unwrap();
        path
    }

    /// 清单认下来了就用它说话：版本压过条目名里的那一串数字，家族压过「什么线索都没有 ⇒ Fabric」
    #[test]
    fn cf_manifest_wins_over_guessing() {
        let path = write_cf_zip(
            CF_ENTRY,
            CF_FORGE,
            &[
                "overrides/mods/minecraft-1.16.5-modkit.jar",
                "overrides/config/x.toml",
            ],
        );
        let parsed = parse(&path);
        let _ = std::fs::remove_file(&path);
        assert!(parsed.manifest.parsed, "{:?}", parsed.manifest.error);
        assert_eq!(parsed.manifest.mc_version, "1.20.1");
        assert_eq!(parsed.manifest.loader, LoaderKind::Forge);
        assert_eq!(parsed.loader_version.as_deref(), Some("47.2.20"));
        // jar 在 `overrides/mods/` 里：物理名留壳（取件按它抽字节），交付位置不带壳
        assert_eq!(parsed.mod_files.len(), 1);
        assert_eq!(
            parsed.mod_files[0].path,
            "overrides/mods/minecraft-1.16.5-modkit.jar"
        );
        assert_eq!(
            parsed.logical_rel("overrides/mods/minecraft-1.16.5-modkit.jar"),
            "mods/minecraft-1.16.5-modkit.jar"
        );
        assert!(parsed.mod_files[0].in_pack);
        let extras: Vec<&str> = parsed.extra_files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(extras, vec!["overrides/config/x.toml"]);
    }

    /// 清单只给家族、没给版本号时，mods 里那枚**同家族**的加载器 jar 配补上版本号
    #[test]
    fn cf_loader_jar_fills_the_version_the_manifest_left_out() {
        let path = write_cf_zip(
            CF_ENTRY,
            r#"{ "minecraft": { "version": "", "modLoaders": [ { "id": "neoforge-" } ] } }"#,
            &["overrides/mods/neoforge-21.1.77-universal.jar"],
        );
        let parsed = parse(&path);
        let _ = std::fs::remove_file(&path);
        assert!(parsed.manifest.parsed, "{:?}", parsed.manifest.error);
        assert_eq!(parsed.manifest.loader, LoaderKind::NeoForge);
        assert_eq!(parsed.loader_version.as_deref(), Some("21.1.77"));
        // 清单没写版本、条目名里也没有 `minecraft`/包文件名线索 ⇒ 照旧留空，不硬猜一档
        assert_eq!(parsed.manifest.mc_version, "");
    }

    /// 家族以清单为准：jar 文件名猜出的是另一家，它的版本号就不能配替补上去
    /// （`fabric-loader-0.16.9.jar` 的版本拿去当 forge 的版本用，等于给下载器一个不存在的坐标）
    #[test]
    fn cf_manifest_and_loader_jar_disagree_keeps_declared_version() {
        let path = write_cf_zip(
            CF_ENTRY,
            CF_FORGE,
            &["overrides/mods/fabric-loader-0.16.9.jar"],
        );
        let parsed = parse(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(parsed.manifest.loader, LoaderKind::Forge);
        assert_eq!(parsed.loader_version.as_deref(), Some("47.2.20"));
    }

    /// `manifest.json` 这名字太通用：内容不像 CF 那份形状就当它不存在，启发式那条腿照旧活着
    #[test]
    fn unrelated_manifest_json_does_not_capture_the_pack() {
        let path = write_cf_zip(
            CF_ENTRY,
            r#"{ "name": "something-else", "version": "2" }"#,
            &["mods/plain.jar"],
        );
        let parsed = parse(&path);
        let _ = std::fs::remove_file(&path);
        assert!(parsed.manifest.parsed, "{:?}", parsed.manifest.error);
        assert_eq!(parsed.manifest.mc_version, "");
        assert_eq!(parsed.manifest.loader, LoaderKind::Fabric);
        assert_eq!(parsed.mod_files.len(), 1);
    }

    /// 官方导出的 CF 包**不带 jar 字节**（`files[]` 只有 projectID/fileID）：按编号出方案行，
    /// 名字/大小/sha1/直链由 `core::cfpack` 联网补 ⇒ 解析这一层必须让它过，而不是报「没内容可转」
    #[test]
    fn cf_declared_ids_become_rows_when_the_pack_has_no_bytes() {
        let path = write_cf_zip(CF_ENTRY, CF_FORGE, &["overrides/config/x.toml"]);
        let parsed = parse(&path);
        let _ = std::fs::remove_file(&path);
        assert!(parsed.manifest.parsed, "{:?}", parsed.manifest.error);
        assert_eq!(parsed.mod_files.len(), 1);
        let row = &parsed.mod_files[0];
        // 路径只是锚点：detector 按 src_path 精确回指，split_mod_file 从它切出 id=1 / version=2
        assert_eq!(row.path, "mods/1-2.jar");
        assert_eq!(row.cf.as_ref().map(|c| (c.mod_id.as_str(), c.file_id.as_str())), Some(("1", "2")));
        assert!(!row.in_pack, "字节不在包里：这一行必须判成要联网取");
        assert_eq!(row.size_bytes, 0, "编号表里没有大小，别拿 0 之外的数当真");
        assert_eq!(parsed.manifest.mod_count, 1);
        // overrides 的内容照旧进保留清单
        let extras: Vec<&str> = parsed.extra_files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(extras, vec!["overrides/config/x.toml"]);
    }

    /// 同一对编号重复列过（手改过的清单真有这种）⇒ 只出一行：一行编号就是方案一行，
    /// 留着重复等于同一个 jar 下两遍
    #[test]
    fn duplicate_cf_ids_collapse_to_one_row() {
        let path = write_cf_zip(
            CF_ENTRY,
            r#"{ "minecraft": { "version": "1.20.1", "modLoaders": [ { "id": "forge-47.2.20" } ] },
                 "files": [ { "projectID": 1, "fileID": 2 }, { "projectID": 1, "fileID": 2 },
                            { "projectID": "1", "fileID": "3" } ] }"#,
            &["overrides/config/x.toml"],
        );
        let parsed = parse(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(parsed.mod_files.len(), 2, "{:?}", parsed.mod_files);
        // 字符串形式的 id 也吃（个别老包给字符串）
        assert_eq!(parsed.mod_files[1].path, "mods/1-3.jar");
    }

    /// 包里躺着 jar 时 `files[]` 不参与：编号与文件名离线对不上号，硬并等于把同一个模组列两遍
    /// （民间 CF 包十有九成是「清单 + overrides/mods 里的字节」，那一档按 P2-a 的口径走）
    #[test]
    fn cf_ids_are_ignored_when_the_pack_carries_jars() {
        let path = write_cf_zip(CF_ENTRY, CF_FORGE, &["overrides/mods/somekit.jar"]);
        let parsed = parse(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(parsed.mod_files.len(), 1);
        assert_eq!(parsed.mod_files[0].file_name, "somekit.jar");
        assert!(parsed.mod_files[0].cf.is_none());
        assert!(parsed.mod_files[0].in_pack);
    }

    /// CF 清单既没声明条目、包里也没有 jar ⇒ 才是真没内容可转
    #[test]
    fn cf_pack_with_neither_ids_nor_bytes_says_so() {
        let path = write_cf_zip(
            CF_ENTRY,
            r#"{ "minecraft": { "version": "1.20.1", "modLoaders": [ { "id": "forge-47.2.20" } ] },
                 "files": [] }"#,
            &["overrides/config/x.toml"],
        );
        let parsed = parse(&path);
        let _ = std::fs::remove_file(&path);
        assert!(!parsed.manifest.parsed);
        assert_eq!(parsed.manifest.error.as_deref(), Some(CF_NOTHING));
    }

    /// 套壳 CF 包：清单躺在外层文件夹里（`MyPack/manifest.json`），jar 在 `MyPack/overrides/mods/`。
    /// 包根按 mods 定位算出来是 `MyPack/overrides/`，清单自己落在那一层外面 ⇒ 得按物理名剔掉，
    /// 不然保留清单里会冒出一枚勾上就把编号表拷进服务端的 `manifest.json`
    #[test]
    fn wrapped_cf_pack_does_not_offer_its_own_manifest() {
        let path = write_cf_zip(
            "MyPack/manifest.json",
            CF_FORGE,
            &[
                "MyPack/overrides/mods/somekit.jar",
                "MyPack/overrides/config/x.toml",
                "MyPack/modlist.html",
            ],
        );
        let parsed = parse(&path);
        let _ = std::fs::remove_file(&path);
        assert!(parsed.manifest.parsed, "{:?}", parsed.manifest.error);
        assert_eq!(parsed.manifest.mc_version, "1.20.1");
        assert_eq!(parsed.root_prefix, "MyPack/overrides/");
        assert_eq!(
            parsed.logical_rel("MyPack/overrides/mods/somekit.jar"),
            "mods/somekit.jar"
        );
        let extras: Vec<&str> = parsed.extra_files.iter().map(|f| f.path.as_str()).collect();
        assert!(!extras.iter().any(|p| p.ends_with("manifest.json")), "{extras:?}");
        // modlist.html 是人读的模组列表、我们不解析它，所以它仍算「包里的内容」，不该被顺手剔掉
        assert!(extras.contains(&"MyPack/modlist.html"), "{extras:?}");
    }

    /// 加载器 jar 的文件名直取：三家各自的官方构建产物形状
    #[test]
    fn loader_jar_names_give_kind_and_version() {
        let v = |k: LoaderKind, s: &str| Some((k, Some(s.to_string())));
        assert_eq!(
            loader_from_jar_name("fabric-loader-0.16.9.jar"),
            v(LoaderKind::Fabric, "0.16.9")
        );
        assert_eq!(
            loader_from_jar_name("Fabric-Loader-0.14.21-1.20.1.JAR"),
            v(LoaderKind::Fabric, "0.14.21")
        );
        assert_eq!(
            loader_from_jar_name("neoforge-21.1.77-universal.jar"),
            v(LoaderKind::NeoForge, "21.1.77")
        );
        assert_eq!(
            loader_from_jar_name("forge-1.20.1-47.2.20-universal.jar"),
            v(LoaderKind::Forge, "47.2.20")
        );
        assert_eq!(
            loader_from_jar_name("forge-1.7.10-10.13.4.1614.jar"),
            v(LoaderKind::Forge, "10.13.4.1614")
        );
        // 省掉 MC 那一段的写法：单看一段就当构建号
        assert_eq!(
            loader_from_jar_name("forge-47.2.20-universal.jar"),
            v(LoaderKind::Forge, "47.2.20")
        );
        // 只有 MC 那一段（installer 那种）⇒ 宁可不给版本，也不把 1.20.1 当构建号塞进下拉
        assert_eq!(
            loader_from_jar_name("forge-1.20.1-installer.jar"),
            Some((LoaderKind::Forge, None))
        );
        // 名字里带这些字样但不是加载器本体：fabric-api 是模组库，jei 那枚是模组的 Forge 版
        assert_eq!(loader_from_jar_name("fabric-api-0.92.2.jar"), None);
        assert_eq!(loader_from_jar_name("jei-1.20.1-forge-15.2.0.jar"), None);
        assert_eq!(loader_from_jar_name("sodium.jar"), None);
    }

    /// 强证据压过弱证据：mods 里躺着 `fabric-loader-*.jar` 时，整包别处的 `forge` 字样不改判，
    /// 而且这一档的版本号不再恒为空（旧写法两个毛病都在：那条配置能把整包定成 Forge，版本只能落到列表首项）
    #[test]
    fn loader_jar_in_mods_outranks_the_substring_scan() {
        let path = std::env::temp_dir().join(format!(
            "sideshift-parser-{}.zip",
            uuid::Uuid::new_v4()
        ));
        let mut w = zip::ZipWriter::new(File::create(&path).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        w.start_file("mods/fabric-loader-0.16.9.jar", opts).unwrap();
        w.write_all(b"PK\x03\x04").unwrap();
        w.start_file("mods/sodium.jar", opts).unwrap();
        w.write_all(b"PK\x03\x04").unwrap();
        w.start_file("config/forgeconfigmodifier.cfg", opts).unwrap();
        w.write_all(b"c").unwrap();
        w.finish().unwrap();

        let parsed = parse(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(parsed.manifest.loader, LoaderKind::Fabric);
        assert_eq!(parsed.loader_version.as_deref(), Some("0.16.9"));
    }

    /// 没有加载器 jar 时退回旧的那档弱证据，且**不给版本号**：老 Forge（1.16 及以下）的包
    /// mods 里本来就没有那一枚，删了这档会把整批包统一误标成 Fabric
    #[test]
    fn substring_scan_still_decides_when_no_loader_jar_is_present() {
        let path = std::env::temp_dir().join(format!(
            "sideshift-parser-{}.zip",
            uuid::Uuid::new_v4()
        ));
        let mut w = zip::ZipWriter::new(File::create(&path).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        w.start_file("mods/jei.jar", opts).unwrap();
        w.write_all(b"PK\x03\x04").unwrap();
        w.start_file("config/neoforge.toml", opts).unwrap();
        w.write_all(b"t").unwrap();
        w.finish().unwrap();

        let parsed = parse(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(parsed.manifest.loader, LoaderKind::NeoForge);
        assert_eq!(parsed.loader_version, None);
    }
}
