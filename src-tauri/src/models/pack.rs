use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LoaderKind {
    Fabric,
    Forge,
    NeoForge,
}

impl LoaderKind {
    pub fn as_label(self) -> &'static str {
        match self {
            LoaderKind::Fabric => "Fabric",
            LoaderKind::Forge => "Forge",
            LoaderKind::NeoForge => "NeoForge",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PackManifest {
    pub file_name: String,
    pub loader: LoaderKind,
    pub mc_version: String,
    pub mod_count: u32,
    pub size_bytes: u64,
    pub parsed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// 源包绝对路径：前端解析得到、创建任务时原样带回，后端据此定位包文件
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
}

impl PackManifest {
    /// 这一包的内存身份：**绝对源路径**，不是文件名。
    /// 同名不同目录的两个包（把同一个包复制一份再改内容，是最常见的情形）在文件名身份下
    /// 是同一个包 ⇒ 后一个包直接复用前一个的端证据与解析缓存，界面上就是「分类好的数据是上一个包的」。
    /// 没有路径的旧存档退回文件名，至少有得比
    pub fn identity(&self) -> String {
        self.source_path.clone().unwrap_or_else(|| self.file_name.clone())
    }
}

/// 保留树里的一个文件条目（弹窗展示用；`keep_files` 的取值 = 它的逻辑相对路径）
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PackFileNode {
    /// 文件名（不含路径），如 options.txt
    pub name: String,
    /// 原始字节；0 = 未知（index 没给 fileSize 且 zip 条目也没测到）
    pub size_bytes: u64,
    /// 从包根起算的逻辑相对路径（已剥 overrides 壳），如 kubejs/client_scripts/keep.js
    pub path: String,
}

/// 包内可保留内容的整棵树（客户端保留内容弹窗数据源）
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct PackDirTree {
    /// 目录树（路径里出现 mods 与 resourcepacks 的子树已在建树时跳过）
    pub dirs: Vec<PackDirNode>,
    /// 包根散文件（`options.txt`、`servers.dat` 这类）；目录里的直属文件挂在各节点 `files` 上，
    /// 两档在弹窗里都可勾，勾选值都是完整逻辑路径
    pub files: Vec<PackFileNode>,
}

/// 包内可保留目录树节点（客户端保留内容弹窗数据源）；
/// keep_dirs 条目 = 从包根起算的相对路径（如 kubejs/client_scripts），前缀匹配拷贝、剪层落位
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PackDirNode {
    /// 目录名（不含路径），如 client_scripts
    pub name: String,
    /// 该目录内文件数（递归，含子目录）
    pub file_count: u32,
    /// 该目录内字节数（递归，含子目录；用于「勾之前先看多大」）
    pub size_bytes: u64,
    /// **直属**文件（不含子目录里的），按名升序；只读展示用，勾选仍走目录前缀
    pub files: Vec<PackFileNode>,
    /// 子目录节点，名字升序
    pub children: Vec<PackDirNode>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct VersionOption {
    pub value: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
}
