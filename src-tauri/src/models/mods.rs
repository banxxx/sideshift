use serde::{Deserialize, Serialize};

use crate::models::{BytecodeHint, EnvSource, LoaderKind, SideFlag};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ModSource {
    Modrinth,
    Curseforge,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ModSearchResult {
    pub id: String,
    /// 平台 URL slug：Modrinth 的 `id` 本来就是它，CurseForge 另给一栏（`id` 是数字 mod id）。
    /// 只有 slug 能进麦块的 `detail/{slug}`，所以中文简介那条线吃这个而不是 `id`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    pub name: String,
    pub description: String,
    pub author: String,
    pub downloads: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_url: Option<String>,
    pub source: ModSource,
    pub compatible: bool,
    pub already_added: bool,
    /// 项目级两侧支持度（Modrinth `client_side`/`server_side`）：在线添加行的端标签
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_side: Option<SideFlag>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_side: Option<SideFlag>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ModSearchPage {
    pub source: ModSource,
    pub total: u64,
    pub results: Vec<ModSearchResult>,
    pub page: u32,
    pub page_size: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ModSearchQuery {
    pub source: ModSource,
    pub text: String,
    /// 空串 = 全部版本（不加 versions facet）
    pub mc_version: String,
    /// None = 任意加载器（不加 categories facet；前端「任意加载器」传 null）
    #[serde(default)]
    pub loader: Option<LoaderKind>,
    #[serde(default)]
    pub category: Option<String>,
    pub page: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ModVersionEntry {
    pub id: String,
    pub version_number: String,
    pub mc_version: String,
    pub loader: LoaderKind,
    pub date: String,
    pub size_bytes: u64,
    pub recommended: bool,
    /// 该构建主文件的直链（在线添加时随版本一起钉住）
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha1: Option<String>,
    /// 服务端下载文件名（与方案行展示版本对应）
    pub file_name: String,
    /// 构建级 `environment` 换算出的两侧支持度：这个构建进服务端包要不要，添加前就能看到
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_side: Option<SideFlag>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_side: Option<SideFlag>,
    /// 该构建声明的前置（依赖）。版本列表接口本就带着这份数据（Modrinth `dependencies[]` /
    /// CF file `dependencies[]`），名字由一次批量反查补上；查不到就只有 id，前端按 id 兜底显示
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends: Vec<ModDepends>,
}

/// 构建的一条前置。`id` 是平台侧项目标识（Modrinth project_id / CurseForge 数字 mod id），
/// `name`/`slug` 是反查回来的显示信息（平台没答上就缺省）
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ModDepends {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    /// required 必装 / optional 可选。incompatible / embedded 不进这张表——
    /// 前者是要避开的关系，后者已经打进 jar 里，都不是「要另装的前置」
    pub required: bool,
}

/// 手动添加那一行的取证结果（本地 jar 与在线构建共用；两侧支持度来自阶梯跑完的那一层）
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct AddedModSide {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_side: Option<SideFlag>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_side: Option<SideFlag>,
    pub env_source: EnvSource,
    /// jar 字节码结构提示（同 `PlanMod::bytecode_hint` 口径）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytecode_hint: Option<BytecodeHint>,
    /// 探测到的实际字节数（本地添加行补体积，摘要才不算空）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
    /// jar 内自报的模组 id（`fabric.mod.json:id` / `mods.toml:modId`）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mod_id: Option<String>,
    /// 自报显示名：文件名被改成中文时这才是可读名字
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// 详情页「翻译」按钮那份中文译文（麦块镜像的 `detail/{slug}`，机器翻译件）。
/// 两个字段各有一档覆盖率：`description_zh` 头部实测基本全有，`title_zh` 只有五成上下，
/// 所以「有译文」的判据是**两者都空才算没有**，且调用方要能只拿到其中一个
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct ModTranslation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title_zh: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description_zh: Option<String>,
}
