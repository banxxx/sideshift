pub mod parser;
pub mod data_root;
pub mod env;
pub mod detector;
pub mod downloader;
pub mod mc_version;
// 内置模组名称词典（离线，与下面那两家在线 API 同层但零请求）
pub mod mcmod_names;
pub mod java;
pub mod installer;
pub mod builder;
pub mod cfpack;
pub mod estimate;
pub mod verify;
pub mod cleanup;
pub mod ack;
// 应用更新：渠道判定、release 解析（下载与安装分两期接进来）
pub mod update;
