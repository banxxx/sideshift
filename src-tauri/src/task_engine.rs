//! 任务引擎：内存任务注册表 + 四阶段流水线调度 + 取消 + 进度事件。
//! 阶段进度口径与前端 mock 引擎一致：parser≤15 · detector≤30 · downloader≤82 · builder≤100。
//!
//! 本文件只是模块根（barrel）：对外仍然用 `crate::task_engine::X` 访问，内部按职责分七块——
//! `state`（注册表与 AppState）· `persist`（settings.json / tasks.json）·
//! `events`（日志环、进度事件、失败落档）· `activity`（分目录日志与实时条账本）·
//! `schedule`（排队与取消）· `pipeline`（四阶段流水线）· `util`（时间与文件名小件）。

mod activity;
mod events;
mod persist;
mod pipeline;
mod schedule;
mod state;
mod util;

pub use events::EVENT_CLASSIFIED;
pub use persist::{save_settings, save_tasks};
pub use schedule::{cancel, create_task, remove_task_staging, retry_task, StartResult};
pub use state::{AppState, Inner};

/// 单条任务的暂存目录名：`{cache}\tasks\{任务 id}\staging`。写它的（pipeline/schedule）与
/// 判它是不是孤儿的（core::cleanup）都引这一个常量，分叉了就是「清理漏掉残留」那种查半天的 bug
pub const CACHE_TASKS_DIR: &str = "tasks";
