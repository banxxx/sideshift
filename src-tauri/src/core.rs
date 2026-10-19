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
pub mod installer_deps;
pub mod builder;
pub mod cfpack;
pub mod estimate;
pub mod verify;
pub mod verify_copy;
pub mod verify_run;
pub mod cleanup;
// 转换任务日志落盘：一轮一份，全量不受界面那条 600 行环约束
pub mod task_log;
pub mod ack;
// 应用更新：渠道判定、release 解析（下载与安装分两期接进来）
pub mod update;
// 卸载壳自愈：安装壳与主应用共用的投递/改指/收存逻辑（#[path] 复用，改一处两处生效）
pub mod uninstall_shell;
