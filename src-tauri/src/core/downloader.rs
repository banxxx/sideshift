//! 下载器与外部 API：并发限流 + sha1 缓存 + 3 次重试；Modrinth / CurseForge 搜索与版本解析；
//! MC 版本表（piston-meta）、Fabric/Forge/NeoForge 加载器版本表。
//! 本文件只是模块根（barrel）：对外仍用 `crate::core::downloader::X` 访问，内部按职责分为
//! types / client / source / offline / modrinth / minekuai / mcmod / curseforge / versions / util 各块。

mod client;
mod curseforge;
mod mcmod;
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
// 百科那条补全腿的采信判据交给 env 层用（页面结构只在这块里解析，别让 HTML 漏出去）
pub(crate) use mcmod::{mcmod_confident, McmodPage};
pub use types::{
    DownloadError, Fetch, FetchSource, ItemSpec, TransferProgress, net_code, reqwest_code,
};
// 缓存布局的两个事实交给清理侧用（core::cleanup）：目录名与半成品判据，写与删共用一份
pub use util::{is_partial_name, CACHE_FILES_DIR};
