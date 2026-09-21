//! 下载器与外部 API：并发限流 + sha1 缓存 + 3 次重试；Modrinth 搜索/版本解析；
//! MC 版本表（piston-meta）、Fabric/Forge/NeoForge 加载器版本表。
//!
//! 本文件只是模块根（barrel）：对外仍然用 `crate::core::downloader::X` 访问，内部按职责分六块——
//! `types`（取件契约与错误）· `client`（Downloader 本体：联网流式取回 + HTTP 原语）·
//! `source`（下载源：官方 URL ↔ BMCLAPI 镜像候选链）·
//! `offline`（离线批量取件：复制分片与单遍解包）· `modrinth`（Modrinth 查询与端声明）·
//! `versions`（MC 与三款加载器的版本表、加载器 jar 坐标）。

mod client;
mod modrinth;
mod offline;
mod source;
mod types;
mod util;
mod versions;

pub use client::Downloader;
pub use modrinth::ModrinthEnv;
pub use types::{DownloadError, Fetch, FetchSource, ItemSpec, TransferProgress};
