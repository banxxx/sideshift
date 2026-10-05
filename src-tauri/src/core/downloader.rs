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
// 名字归一化两份账共用：联网那条百科腿的「同形」判据与内置词典的中文键必须是同一个算法
pub(crate) use mcmod::norm_name;
pub use types::{
    DownloadError, Fetch, FetchSource, ItemSpec, TransferProgress, app_code, net_code,
    net_timeout_code, reqwest_code,
};
// 半成品判据交给清理侧用（core::cleanup）：写与删共用一份。缓存目录名不在这里，单源是
// `core::data_root` 那张表（卸载壳的删除清单引的是同一份）
pub use util::is_partial_name;
// 应用更新那条链借三件现成的事实：同一个 UA（GitHub 侧认的是同一个应用）、
// 半成品后缀与种类码出口。它自己不走 Downloader——那个客户端设了整请求 120s 上限，
// 十几 MB 的包在弱网下本来就该跑几分钟（见 core::update::fetch）
pub(crate) use client::USER_AGENT;
pub(crate) use util::PART_MARKER;
