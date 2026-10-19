//! 转换任务的日志文件：一轮一份，每行即时追写，全量不受界面那条 600 行环的约束。
//! 位置 `{cache}\tasks\{任务 id}\logs\{YYYY-MM-DD-HHMMSS}.log` ⇒ 与暂存同目录、任务删了它就没；
//! 而设置页那两颗清理钮只动 `files`/`update` 两桶和「注册表里已无此 id」的孤儿目录，扫不到在册任务的这一份。
//! 写完即关（不常驻句柄、不攒批）：进程被强杀时，已经落盘的那几行不会跟着丢。

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::core::data_root;
use crate::models::{LogLevel, PipelineStage, TaskLogLine};

/// 任务目录里那一层日志文件夹的名字。它不是缓存桶（躺在 `tasks\{id}\` 里面），所以不进
/// `data_root::CACHE_OWNED` 那张卸载清单——删任务目录连它一起删
pub const LOG_DIR_NAME: &str = "logs";
/// 定宽零填充的轮次时间戳：字典序就是时间序，`latest` 靠这一点挑出最新那一份
const STAMP_FMT: &str = "%Y-%m-%d-%H%M%S";
/// 上面那串展开出来的字符数（`2026-10-18-142203`）：认文件名时只解这一段，后面的 `_2` 让位段不参与
const STAMP_LEN: usize = 17;

/// `{cache}\tasks\{id}\logs`。id 不是我们生成过的那形状（见 `data_root::is_task_id`）就没有这一层
pub fn dir_for(cache_dir: &Path, task_id: &str) -> Option<PathBuf> {
    if !data_root::is_task_id(task_id) {
        return None;
    }
    Some(
        cache_dir
            .join(data_root::CACHE_TASKS_DIR)
            .join(task_id)
            .join(LOG_DIR_NAME),
    )
}

/// 开一轮：建目录、定下这一份的文件名，返回绝对路径。建不出来返回 None ⇒
/// 这一轮只有界面那条日志环，转换照跑（日志落盘是旁路，不是前置件）
pub fn open_round(cache_dir: &Path, task_id: &str) -> Option<PathBuf> {
    let dir = dir_for(cache_dir, task_id)?;
    fs::create_dir_all(&dir).ok()?;
    Some(pick_name(&dir, &chrono::Local::now().format(STAMP_FMT).to_string()))
}

/// 同一秒里开两轮（重试撞在秒边界上）就让位。分隔符用 `_` 而不是 `-`：`_`(0x5F) 排在
/// `.log` 那个点(0x2E) 后面，`latest` 才认得出让位后的那份更新
fn pick_name(dir: &Path, stamp: &str) -> PathBuf {
    let plain = dir.join(format!("{stamp}.log"));
    if !plain.exists() {
        return plain;
    }
    (2u32..)
        .map(|n| dir.join(format!("{stamp}_{n}.log")))
        .find(|p| !p.exists())
        .unwrap_or(plain)
}

/// 追写一行：文件不在就当场建，写完即关。失败不报——一条日志写不下去不该让转换失败
pub fn append(path: &Path, line: &TaskLogLine) {
    let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    let _ = writeln!(f, "{}", line_of(line));
}

/// 文件里的那一行：`[09:41:02] [warn] [installer] 本机安装 Forge …`。
/// 两个标签用与 `tasks.json` 同一套小写名（界面按同一套名字映射出本地化文字），
/// 文案仍是那句中文整句 `message`——与存档、README 同口径
fn line_of(line: &TaskLogLine) -> String {
    format!(
        "[{}] [{}] [{}] {}",
        line.time,
        level_tag(line.level),
        stage_tag(line.stage),
        line.message
    )
}

fn level_tag(level: LogLevel) -> &'static str {
    match level {
        LogLevel::Info => "info",
        LogLevel::Warn => "warn",
        LogLevel::Error => "error",
    }
}

fn stage_tag(stage: PipelineStage) -> &'static str {
    match stage {
        PipelineStage::Parser => "parser",
        PipelineStage::Detector => "detector",
        PipelineStage::Downloader => "downloader",
        PipelineStage::Installer => "installer",
        PipelineStage::Builder => "builder",
    }
}

/// 这一份任务**最新**的那一份日志（冷启动后内存里没有轮次路径，只能扫目录认）。
/// 只认本模块写出去的那个形状，用户自己丢进那层的文件不算
pub fn latest(cache_dir: &Path, task_id: &str) -> Option<PathBuf> {
    let dir = dir_for(cache_dir, task_id)?;
    let entries = fs::read_dir(&dir).ok()?;
    let mut best: Option<(String, PathBuf)> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        let is_log = path.extension().and_then(|e| e.to_str()) == Some("log");
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()).map(ToOwned::to_owned) else {
            continue;
        };
        if !is_log || !is_round_name(&stem) {
            continue;
        }
        if best.as_ref().is_none_or(|(name, _)| name.as_str() <= stem.as_str()) {
            best = Some((stem, path));
        }
    }
    best.map(|(_, path)| path)
}

/// 文件名是不是我们写出去的那形状：前 17 位按定宽时间戳解得开（后面的 `_2` 让位段不参与判据）
fn is_round_name(stem: &str) -> bool {
    let head = match stem.get(..STAMP_LEN) {
        Some(h) => h,
        None => return false,
    };
    chrono::NaiveDateTime::parse_from_str(head, STAMP_FMT).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sideshift-tasklog-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 任务 id 从前端来，而这里每条路径都拼它 ⇒ 带分隔符、`..`、盘符的一律不拼
    #[test]
    fn bogus_task_ids_never_become_a_path() {
        let cache = PathBuf::from("C:\\cache");
        let long = "x".repeat(65);
        for id in ["", "..", "..\\..\\windows", "task-1/staging", "a\\b", long.as_str()] {
            assert_eq!(dir_for(&cache, id), None, "{id} 不该被拼成路径");
        }
        assert!(dir_for(&cache, "task-20261018-abcd").is_some());
    }

    /// 一轮一份：同一秒里再开一轮就换名，且换名后那份仍排在旧的一份之后
    #[test]
    fn a_colliding_stamp_gets_a_suffix_that_sorts_last() {
        let root = temp_root("stamp");
        let dir = root.join("one");
        fs::create_dir_all(&dir).unwrap();
        let stamp = "2026-10-18-142203";
        let plain = dir.join(format!("{stamp}.log"));
        assert_eq!(pick_name(&dir, stamp), plain);
        fs::File::create(&plain).unwrap();
        let second = pick_name(&dir, stamp);
        assert_eq!(second, dir.join(format!("{stamp}_2.log")));
        assert!(
            second.to_string_lossy() > plain.to_string_lossy(),
            "让位那份的文件名必须排在原始那份后面，否则 latest 会挑到旧的"
        );
        fs::remove_dir_all(root).ok();
    }

    /// 行形状钉死：时间、级别、阶段三个标签 + 中文整句，与 tasks.json 里那套小写名同名
    #[test]
    fn one_line_is_three_tags_and_the_message() {
        let line = TaskLogLine {
            time: "09:41:02".into(),
            stage: PipelineStage::Downloader,
            message: "取件 · config · 12 个".into(),
            message_msg: None,
            level: LogLevel::Warn,
        };
        assert_eq!(line_of(&line), "[09:41:02] [warn] [downloader] 取件 · config · 12 个");
    }

    /// latest 只认自己的形状，并按字典序取最新那一份（定宽零填充 ⇒ 字典序即时间序）
    #[test]
    fn latest_returns_the_newest_round_file() {
        let root = temp_root("latest");
        let cache = root.join("cache");
        let dir = dir_for(&cache, "task-abc").unwrap();
        fs::create_dir_all(&dir).unwrap();
        for name in [
            "2026-10-17-080000.log",
            "2026-10-18-142203.log",
            "2026-10-18-142203_2.log",
            "notes.txt",
            "zzz-user-written.log",
        ] {
            fs::write(dir.join(name), "x").unwrap();
        }
        assert_eq!(
            latest(&cache, "task-abc").unwrap(),
            dir.join("2026-10-18-142203_2.log")
        );
        fs::remove_dir_all(root).ok();
    }

    /// 没开过轮（目录不存在）返回 None：界面上那颗钮据此说「还没写下日志文件」
    #[test]
    fn no_directory_means_no_log_file() {
        let cache = temp_root("absent").join("cache");
        assert_eq!(latest(&cache, "task-abc"), None);
    }

    /// 追写会当场建父路径之外的文件，并且每行追加不覆盖（重试另起一份，同一轮内是追加）
    #[test]
    fn append_creates_then_adds() {
        let root = temp_root("append");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("2026-10-18-142203.log");
        let line = TaskLogLine {
            time: "09:41:02".into(),
            stage: PipelineStage::Parser,
            message: "解析完成".into(),
            message_msg: None,
            level: LogLevel::Info,
        };
        append(&path, &line);
        append(&path, &line);
        assert_eq!(fs::read_to_string(&path).unwrap().lines().count(), 2);
        fs::remove_dir_all(root).ok();
    }
}
