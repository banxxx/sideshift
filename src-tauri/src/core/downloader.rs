//! 下载器与外部 API：并发限流 + sha1 缓存 + 3 次重试；Modrinth / CurseForge 搜索与版本解析；
//! MC 版本表（piston-meta）、Fabric/Forge/NeoForge 加载器版本表。
//!
//! 本文件只是模块根（barrel）：对外仍然用 `crate::core::downloader::X` 访问，内部按职责分八块——
//! `types`（取件契约与错误）· `client`（Downloader 本体：联网流式取回 + HTTP 原语）·
//! `source`（下载源：官方 URL ↔ BMCLAPI 镜像候选链）·
//! `offline`（离线批量取件：复制分片与单遍解包）· `modrinth`（Modrinth 查询与端声明）·
//! `minekuai`（麦块开放 API：Modrinth 项目目录的国内镜像，只给端判定加速）·
//! `curseforge`（CurseForge Core API：需要用户自己的 API Key）·
//! `versions`（MC 与三款加载器的版本表、加载器 jar 坐标）。

mod client;
mod curseforge;
mod minekuai;
mod modrinth;
mod offline;
mod source;
mod types;
mod util;
mod versions;

pub use client::Downloader;
pub use curseforge::CfFileMeta;
pub use modrinth::ModrinthEnv;
pub use types::{
    DownloadError, Fetch, FetchSource, ItemSpec, TransferProgress, net_code, reqwest_code,
};
// 缓存布局的两个事实交给清理侧用（core::cleanup）：目录名与半成品判据，写与删共用一份
pub use util::{is_partial_name, CACHE_FILES_DIR};
