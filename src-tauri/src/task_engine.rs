//! 任务引擎：内存任务注册表 + 四阶段流水线调度 + 取消 + 进度事件。
//! 阶段进度口径与前端 mock 引擎一致：parser≤15 · detector≤30 · downloader≤82 · builder≤100。
//!
//! 本文件只是模块根（barrel），按职责分八块：state · persist · events · activity ·
//! schedule · pipeline · trash · util；对外仍用 `crate::task_engine::X` 访问。

mod activity;
mod events;
mod persist;
mod pipeline;
mod schedule;
mod state;
mod trash;
mod util;

pub use events::EVENT_CLASSIFIED;
pub(crate) use persist::config_dir;
pub(crate) use persist::webview_profile_dir;
pub use persist::{save_settings, save_templates};
pub use schedule::{cancel, create_task, remove_task_staging, retry_task, StartResult};
pub use state::{has_active_tasks, AppState, Inner};
pub use trash::{drain_trash, entries as trash_entries, restore_task, trash_task, TrashEntry};

/// 单条任务的暂存目录名：`{cache}\tasks\{任务 id}\staging`。写它的（pipeline/schedule）与
/// 判它是不是孤儿的（core::cleanup）都引这一个常量，分叉了就是「清理漏掉残留」那种查半天的 bug
pub const CACHE_TASKS_DIR: &str = "tasks";
