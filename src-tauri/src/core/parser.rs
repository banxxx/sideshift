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
pub(super) fn strip_root_prefix<'a>(prefix: &str, rel: &'a str) -> &'a str {
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
pub(super) fn zip_names(path: &Path) -> Result<Vec<String>, String> {
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
pub(super) fn find_manifest(names: &[String], wanted: &str) -> Option<(String, String)> {
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


mod curseforge;
mod mrpack;
mod scan;
#[cfg(test)]
mod tests;

use curseforge::{cf_declared_facts, Declared};
use mrpack::parse_mrpack;
use scan::scan_zip_entries;
