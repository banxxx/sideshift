//! 缓存占用统计与手动清理。
//! - 统计与删除共用**同一个分类函数**（`snapshot`），两边不得各写各的判据。
//! - 扫描是纯读，不顺手删任何东西——回收量与按钮语义都由用户显式触发。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::core::downloader::{CACHE_FILES_DIR, is_partial_name};
use crate::core::update::fetch::{CACHE_UPDATE_DIR, busy as update_running};
use crate::task_engine::CACHE_TASKS_DIR;
use crate::models::{CacheUsage, CleanReport};

/// 「未使用」的天数门槛。缓存文件的 mtime 在每次命中复用时都会被刷新
/// （`downloader::util::mark_used`），所以这里读到的是「最后一次用」，不是「最后一次下」。
pub const STALE_DAYS: u64 = 30;

/// 下载缓存的清理口径
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanMode {
    /// 只清过期（最后一次使用早于 STALE_DAYS 天前）
    Stale,
    /// 清空全部下载缓存
    All,
}

/// 缓存里的东西按六类分完，一个文件只进一类。类别都对得上一个写入点，不是凭经验猜的：
///  - `fresh` / `stale` —— 正常下载缓存，按最后一次使用时间二分（跨任务复用，删了要重新联网）
///  - `parts` —— 半截下载（`{名}.part{尝试序号}`，见 downloader 的 `temp_path`）。正常失败路径
///    会自己删掉，只有进程被强杀才留得下来
///  - `orphans` —— `cache\tasks\{id}` 且注册表里已无此 id：上次崩溃漏掉的暂存目录。启动时
///    `sweep_task_staging` 也会删，所以这里通常是 0——显示 0 就是真话
///  - `update` —— `cache\update\{版本}`：应用更新那一轮的暂存（安装包 + 同名签名）。**按版本目录
///    计一项**，一次下载是两三个文件，报「删了 3 个文件」不如报「收掉了 1.1.0 这一版」
///  - `empty_dirs` —— 文件删光后剩下的壳目录：零字节，但用户会当成「没清干净」
struct Snapshot {
    fresh: Vec<(PathBuf, u64)>,
    stale: Vec<(PathBuf, u64)>,
    parts: Vec<(PathBuf, u64)>,
    /// (目录, 目录内字节合计)
    orphans: Vec<(PathBuf, u64)>,
    /// (版本目录, 定稿文件字节合计, 里面最新的 mtime)——mtime 是 `stale` 档的判据
    update: Vec<(PathBuf, u64, SystemTime)>,
    empty_dirs: Vec<PathBuf>,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            fresh: Vec::new(),
            stale: Vec::new(),
            parts: Vec::new(),
            orphans: Vec::new(),
            update: Vec::new(),
            empty_dirs: Vec::new(),
        }
    }
}

/// `live_task_ids` = 注册表里全部任务 id（排队/运行中/已完成的历史都算）：它们的暂存目录
/// 是在用文件，重试还要用同一套坐标。
/// `busy` = 有任务正在跑：那时的 `.part` 可能正被写着，既不计入垃圾也不删。
/// `update_writing` = 更新那一轮正在取件。它**不并进 `busy`**：那面旗说的是转换任务，
/// 而更新下载不在任务表里，两处共用一个 bool 只会让「有任务在跑」这句谎多一处出口。
fn snapshot(
    cache_dir: &Path,
    live_task_ids: &BTreeSet<String>,
    busy: bool,
    update_writing: bool,
) -> Snapshot {
    let mut s = Snapshot::default();
    if !cache_dir.is_dir() {
        return s;
    }
    let cutoff = stale_cutoff();
    walk_cache_files(&cache_dir.join(CACHE_FILES_DIR), cutoff, busy, &mut s);
    walk_cache_tasks(cache_dir, live_task_ids, &mut s);
    walk_cache_update(cache_dir, update_writing, &mut s);
    s
}

/// 第 6 类：应用更新的暂存目录 `cache\update\{版本}\`。
///
/// 里面那个 `.part` 归 `parts` 那格，但**忙不忙要问更新那一轮自己**（`update_writing`）：
/// 它不在任务表里，`busy` 那面旗照不到它。让它把正在写的半截报成垃圾、再删不动地失败一次，
/// 界面上就成了「清了但没清干净」。
fn walk_cache_update(cache_dir: &Path, writing: bool, s: &mut Snapshot) {
    let root = cache_dir.join(CACHE_UPDATE_DIR);
    let Ok(rd) = std::fs::read_dir(&root) else {
        return;
    };
    let mut shells = 0usize;
    let mut holdouts = 0usize;
    for e in rd.flatten() {
        let dir = e.path();
        let Ok(meta) = e.metadata() else {
            holdouts += 1;
            continue;
        };
        if !meta.is_dir() {
            // 直接摆在 update\ 下的散文件不属于这套布局，也不归我们删
            holdouts += 1;
            continue;
        }
        let mut bytes = 0u64;
        let mut newest = SystemTime::UNIX_EPOCH;
        let mut staged = 0usize;
        let Ok(inner) = std::fs::read_dir(&dir) else {
            holdouts += 1;
            continue;
        };
        for c in inner.flatten() {
            let path = c.path();
            let Ok(cm) = c.metadata() else { continue };
            if !cm.is_file() {
                continue;
            }
            if is_partial_name(&file_name(&path)) {
                if !writing {
                    s.parts.push((path, cm.len()));
                }
                continue;
            }
            staged += 1;
            bytes += cm.len();
            if let Ok(t) = cm.modified() {
                newest = newest.max(t);
            }
        }
        // 只有半截或空的目录：文件已经进 parts，这一层壳交给 empty_dirs 那格收尾
        if staged == 0 {
            if is_empty_dir(&dir) {
                s.empty_dirs.push(dir);
                shells += 1;
            } else {
                // 里面还躺着正在写（或读不动）的半截：本轮收不掉它，也就别指望根壳能空
                holdouts += 1;
            }
            continue;
        }
        // 过期与否由 `update` 那一格自己带的时间戳决定（`clean_cache` 读它），
        // 不并进 `stale` 那串：那串是一条文件路径一个地删的，目录混进去就是删不动的 failed
        s.update.push((dir, bytes, newest));
    }
    // 根壳排在版本壳**之后**入队，`clean_junk` 按入队顺序删，所以同一趟里子壳先没、根壳随后才被判空。
    // 「只删一层、删前重新看过空」的规矩没破：prune_empty 仍然只在当场确实空了才动手
    if s.update.is_empty() && holdouts == 0 && (shells > 0 || is_empty_dir(&root)) {
        s.empty_dirs.push(root);
    }
}

fn walk_cache_files(root: &Path, cutoff: SystemTime, busy: bool, s: &mut Snapshot) {
    let Ok(rd) = std::fs::read_dir(root) else {
        return;
    };
    for e in rd.flatten() {
        let path = e.path();
        let Ok(meta) = e.metadata() else { continue };
        if !meta.is_dir() {
            // files 根下面直接摆的文件不属于当前布局（老版本或手工放的），按垃圾算
            if !busy && is_partial_name(&file_name(&path)) {
                s.parts.push((path, meta.len()));
            }
            continue;
        }
        let Ok(inner) = std::fs::read_dir(&path) else {
            continue;
        };
        let mut files_here = 0usize;
        for c in inner.flatten() {
            let cp = c.path();
            let Ok(cm) = c.metadata() else { continue };
            if !cm.is_file() {
                continue;
            }
            files_here += 1;
            let name = file_name(&cp);
            let bytes = cm.len();
            if is_partial_name(&name) {
                if !busy {
                    s.parts.push((cp, bytes));
                }
            } else if cm.modified().is_ok_and(|t| t < cutoff) {
                s.stale.push((cp, bytes));
            } else {
                s.fresh.push((cp, bytes));
            }
        }
        // 缓存布局是 files\{键}\{文件名}：这一层的空壳正是「清过但没收尾」的痕迹
        if files_here == 0 {
            s.empty_dirs.push(path);
        }
    }
}

fn walk_cache_tasks(cache_dir: &Path, live_task_ids: &BTreeSet<String>, s: &mut Snapshot) {
    let root = cache_dir.join(CACHE_TASKS_DIR);
    let Ok(rd) = std::fs::read_dir(&root) else {
        return;
    };
    let mut names: Vec<String> = Vec::new();
    for e in rd.flatten() {
        if e.path().is_dir() {
            names.push(e.file_name().to_string_lossy().to_string());
        }
    }
    let mut kept = 0usize;
    for name in names {
        let dir = root.join(&name);
        if live_task_ids.contains(&name) {
            kept += 1;
            continue;
        }
        let bytes = dir_bytes(&dir);
        s.orphans.push((dir, bytes));
    }
    // 在册的都没了 → tasks 这层壳也没用了。注意「本来就没有 tasks 目录」时 read_dir
    // 已经提前返回，不会把用户刚建好的空缓存目录报成垃圾
    if kept > 0 || !s.orphans.is_empty() {
        return;
    }
    if is_empty_dir(&root) {
        s.empty_dirs.push(root);
    }
}

/// 设置页显示的占用报表（纯读，不写盘）
pub fn usage(cache_dir: &Path, live_task_ids: &BTreeSet<String>, busy: bool) -> CacheUsage {
    let s = snapshot(cache_dir, live_task_ids, busy, update_running());
    let sum = |v: &[(PathBuf, u64)]| v.iter().map(|(_, b)| *b).sum::<u64>();
    CacheUsage {
        cache_dir: cache_dir.display().to_string(),
        exists: cache_dir.is_dir(),
        files_count: s.fresh.len() + s.stale.len(),
        files_bytes: sum(&s.fresh) + sum(&s.stale),
        stale_count: s.stale.len(),
        stale_bytes: sum(&s.stale),
        parts_count: s.parts.len(),
        parts_bytes: sum(&s.parts),
        orphan_count: s.orphans.len(),
        orphan_bytes: sum(&s.orphans),
        update_count: s.update.len(),
        update_bytes: s.update.iter().map(|(_, b, _)| *b).sum(),
        empty_dirs: s.empty_dirs.len(),
        busy,
        stale_days: STALE_DAYS,
    }
}

/// 清理垃圾：半截下载 + 孤儿暂存 + 空壳目录。下载缓存本体一个字节都不碰。
pub fn clean_junk(cache_dir: &Path, live_task_ids: &BTreeSet<String>, busy: bool) -> CleanReport {
    let mut s = snapshot(cache_dir, live_task_ids, busy, update_running());
    let mut r = CleanReport::default();
    for (path, bytes) in s.parts.drain(..) {
        remove_cached_file(&path, bytes, &mut r);
    }
    for (dir, bytes) in s.orphans.drain(..) {
        // 一个暂存目录算一项：它内部可能有几百个文件，逐个报数会让人以为删了几百个包
        if std::fs::remove_dir_all(&dir).is_ok() {
            r.items += 1;
            r.bytes += bytes;
        } else {
            r.failed += 1;
        }
    }
    for dir in s.empty_dirs.drain(..) {
        prune_empty(&dir, &mut r);
    }
    r
}

/// 清理下载缓存。删除前重新扫一遍——用户可能在设置页停留期间又跑完了一次转换。
pub fn clean_cache(cache_dir: &Path, mode: CleanMode) -> CleanReport {
    clean_cache_with(cache_dir, mode, update_running())
}

/// 同一件事，但「更新那一轮是否在取件」由调用方给出。测试要能造出「正在写」这一档，
/// 而那颗旗是进程级的、单元测试并行跑 ⇒ 去拧真旗等于给别的测试埋雷
fn clean_cache_with(cache_dir: &Path, mode: CleanMode, writing: bool) -> CleanReport {
    // 空任务名单：这条不动 tasks 桶
    let mut s = snapshot(cache_dir, &BTreeSet::new(), false, writing);
    let mut list = match mode {
        CleanMode::Stale => std::mem::take(&mut s.stale),
        CleanMode::All => {
            let mut all = std::mem::take(&mut s.fresh);
            all.append(&mut s.stale);
            all
        }
    };
    let mut r = CleanReport::default();
    for (path, bytes) in list.drain(..) {
        remove_cached_file(&path, bytes, &mut r);
    }
    // 第 6 类整只按版本目录删：留半个包（zip 有了、`.sig` 没了）比全删更糟——
    // 那一半下次点更新时会被验出来，然后当场删掉，用户只看见「白留了几十 MB」。
    // 正在取件那一版不碰：删目录只是让那次下载当场失败，用户看到的是「点了清理，更新坏了」
    let cutoff = stale_cutoff();
    for (dir, bytes, newest) in s.update.drain(..) {
        if writing {
            continue;
        }
        if mode == CleanMode::Stale && newest >= cutoff {
            continue;
        }
        if std::fs::remove_dir_all(&dir).is_ok() {
            r.items += 1;
            r.bytes += bytes;
        } else {
            r.failed += 1;
        }
    }
    r
}

/// 删一个包内文件，并顺手收掉它那一层壳目录（布局是 `files\{键}\{文件名}`）。
/// 不收的话，清完一次缓存磁盘上就留几千个空目录，「无用文件」那行立刻又多出几千项。
fn remove_cached_file(path: &Path, bytes: u64, r: &mut CleanReport) {
    remove_file(path, bytes, r);
    if let Some(parent) = path.parent() {
        prune_empty(parent, r);
    }
}

fn remove_file(path: &Path, bytes: u64, r: &mut CleanReport) {
    match std::fs::remove_file(path) {
        Ok(()) => {
            r.items += 1;
            r.bytes += bytes;
        }
        // 被占用（杀软攥着句柄、只读位）→ 计入 failed，不虚报释放量，其余继续
        Err(_) => r.failed += 1,
    }
}

/// 确实是空才删，且只删这一层。绝不顺着父链递归：刚删完文件时句柄可能还没释放，
/// 一路往上删会误伤还在用的上层目录。
fn prune_empty(dir: &Path, r: &mut CleanReport) {
    if is_empty_dir(dir) && std::fs::remove_dir(dir).is_ok() {
        r.items += 1;
    }
}

fn is_empty_dir(dir: &Path) -> bool {
    match std::fs::read_dir(dir) {
        Ok(mut rd) => rd.next().is_none(),
        // 读不动就当它不空：宁可留一个空目录，也不删一个看不清内容的目录
        Err(_) => false,
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default()
}

/// 目录内字节合计。深度不固定，用显式栈——保留目录动辄几百个文件，不拿栈空间赌
fn dir_bytes(dir: &Path) -> u64 {
    let mut stack = vec![dir.to_path_buf()];
    let mut total = 0u64;
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let Ok(meta) = e.metadata() else { continue };
            if meta.is_dir() {
                stack.push(e.path());
            } else {
                total += meta.len();
            }
        }
    }
    total
}

fn stale_cutoff() -> SystemTime {
    SystemTime::now() - Duration::from_secs(STALE_DAYS * 24 * 60 * 60)
}

/// 把 mtime 挪到过去：过期判据读的就是它，测试不挪就没法造出「30 天没用」的文件
#[cfg(test)]
fn set_mtime_days_ago(path: &Path, days: u64) {
    let t = SystemTime::now() - Duration::from_secs(days * 24 * 60 * 60);
    let f = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    f.set_modified(t).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_cache(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sideshift-cleanup-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 按真实布局放一个缓存文件：files\{键}\{名}
    fn put(dir: &Path, key: &str, name: &str, bytes: usize, days_ago: u64) -> PathBuf {
        let p = dir.join(CACHE_FILES_DIR).join(key).join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, vec![b'x'; bytes]).unwrap();
        if days_ago > 0 {
            set_mtime_days_ago(&p, days_ago);
        }
        p
    }

    fn staging(dir: &Path, id: &str, bytes: usize) -> PathBuf {
        let d = dir.join(CACHE_TASKS_DIR).join(id).join("staging");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("m.jar"), vec![b'x'; bytes]).unwrap();
        dir.join(CACHE_TASKS_DIR).join(id)
    }

    #[test]
    fn stale_and_fresh_split_by_last_use() {
        let dir = temp_cache("stale");
        put(&dir, "aaa", "new.jar", 10, 1);
        put(&dir, "bbb", "old.jar", 20, STALE_DAYS + 5);

        let u = usage(&dir, &BTreeSet::new(), false);
        assert_eq!((u.files_count, u.files_bytes), (2, 30));
        assert_eq!(
            (u.stale_count, u.stale_bytes),
            (1, 20),
            "只有越过门槛的那个算过期"
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn partials_are_junk_only_when_idle() {
        let dir = temp_cache("parts");
        put(&dir, "aaa", "a.jar", 10, 0);
        std::fs::write(
            dir.join(CACHE_FILES_DIR).join("aaa").join("a.jar.part2"),
            vec![b'x'; 7],
        )
        .unwrap();

        let idle = usage(&dir, &BTreeSet::new(), false);
        assert_eq!((idle.parts_count, idle.parts_bytes), (1, 7));

        // 有任务在跑：那个 .part 可能正是它此刻在写的半成品，不报数也不删
        let busy = usage(&dir, &BTreeSet::new(), true);
        assert_eq!(busy.parts_count, 0);
        assert_eq!(
            clean_junk(&dir, &BTreeSet::new(), true).items,
            0,
            "运行中不清半截下载"
        );
        assert!(dir.join(CACHE_FILES_DIR).join("aaa").join("a.jar.part2").exists());

        let r = clean_junk(&dir, &BTreeSet::new(), false);
        assert_eq!((r.items, r.bytes), (1, 7));
        assert!(
            dir.join(CACHE_FILES_DIR).join("aaa").join("a.jar").exists(),
            "垃圾清理不碰缓存本体"
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn junk_clean_keeps_live_task_staging() {
        let dir = temp_cache("orphans");
        let live = staging(&dir, "live", 5);
        let gone = staging(&dir, "gone", 6);
        let mut ids = BTreeSet::new();
        ids.insert("live".to_string());

        let u = usage(&dir, &ids, false);
        assert_eq!(
            (u.orphan_count, u.orphan_bytes),
            (1, 6),
            "只有注册表里查无此 id 的算孤儿"
        );

        let r = clean_junk(&dir, &ids, false);
        assert_eq!((r.items, r.bytes), (1, 6));
        assert!(live.exists(), "在册任务的暂存不能碰");
        assert!(!gone.exists());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn cache_clean_prunes_emptied_key_dirs() {
        let dir = temp_cache("prune");
        put(&dir, "aaa", "a.jar", 10, 0);
        put(&dir, "bbb", "b.jar", 20, STALE_DAYS + 1);

        let r = clean_cache(&dir, CleanMode::Stale);
        assert_eq!(
            (r.items, r.bytes),
            (2, 20),
            "删文件 + 顺手收掉那层空壳，各算一项"
        );
        assert!(!dir.join(CACHE_FILES_DIR).join("bbb").exists());
        assert!(dir.join(CACHE_FILES_DIR).join("aaa").join("a.jar").exists());

        let r = clean_cache(&dir, CleanMode::All);
        assert_eq!(r.bytes, 10);
        assert_eq!(usage(&dir, &BTreeSet::new(), false).files_count, 0);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn empty_dirs_are_reported_then_collected() {
        let dir = temp_cache("empty");
        std::fs::create_dir_all(dir.join(CACHE_FILES_DIR).join("dead")).unwrap();

        let u = usage(&dir, &BTreeSet::new(), false);
        assert_eq!(u.empty_dirs, 1, "空壳目录要外显，否则用户以为没清干净");
        assert_eq!(u.files_count, 0);

        assert_eq!(clean_junk(&dir, &BTreeSet::new(), false).items, 1);
        assert_eq!(usage(&dir, &BTreeSet::new(), false).empty_dirs, 0);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn missing_cache_dir_reports_zero_instead_of_failing() {
        let dir = std::env::temp_dir().join("sideshift-cleanup-absent-definitely-not-here");
        let _ = std::fs::remove_dir_all(&dir);
        let u = usage(&dir, &BTreeSet::new(), false);
        assert!(!u.exists);
        assert_eq!(u.files_bytes, 0);
        assert_eq!(u.empty_dirs, 0);
        assert_eq!(clean_junk(&dir, &BTreeSet::new(), false).items, 0);
    }

    /// 按更新那一轮的布局放一版暂存：`cache\update\{版本}\{名}`
    fn update_dir(dir: &Path, version: &str, files: &[(&str, usize)], days_ago: u64) -> PathBuf {
        let d = dir.join(CACHE_UPDATE_DIR).join(version);
        std::fs::create_dir_all(&d).unwrap();
        for (name, bytes) in files {
            let p = d.join(name);
            std::fs::write(&p, vec![b'x'; *bytes]).unwrap();
            if days_ago > 0 {
                set_mtime_days_ago(&p, days_ago);
            }
        }
        d
    }

    /// 第 6 类按**版本目录**计一项：一次下载是包 + 同名签名两个文件，
    /// 报「2 个文件」会让人以为清了两次
    #[test]
    fn update_dirs_count_as_versions_not_files() {
        let dir = temp_cache("update-count");
        update_dir(
            &dir,
            "1.1.0",
            &[("SideShift_1.1.0_x64-setup.exe", 30), ("SideShift_1.1.0_x64-setup.exe.sig", 4)],
            1,
        );
        update_dir(&dir, "1.0.0", &[("old.zip", 5)], 1);

        let u = usage(&dir, &BTreeSet::new(), false);
        assert_eq!((u.update_count, u.update_bytes), (2, 39));
        assert_eq!(u.files_count, 0, "更新的暂存不混进下载缓存那格");

        // 里面只剩半截和空壳：文件进 parts，那层壳交给 empty_dirs
        let torn = update_dir(&dir, "1.2.0", &[], 0);
        std::fs::write(torn.join("pkg.zip.part1"), vec![b'x'; 6]).unwrap();
        let u = usage(&dir, &BTreeSet::new(), false);
        assert_eq!(u.update_count, 2, "没收下一个字节的版本不算占用");
        assert_eq!(u.parts_count, 1);
        std::fs::remove_dir_all(dir).ok();
    }

    /// 那一版正在取件时，半截既不计入垃圾也不删——判据是更新自己的旗，不是「有任务在跑」
    #[test]
    fn update_partials_follow_the_update_flag() {
        let dir = temp_cache("update-parts");
        let d = update_dir(&dir, "1.1.0", &[("pkg.zip", 10)], 0);
        std::fs::write(d.join("pkg.zip.part1"), vec![b'x'; 6]).unwrap();

        let idle = snapshot(&dir, &BTreeSet::new(), false, false);
        assert_eq!(idle.parts.len(), 1, "闲下来的半截是没跑完的垃圾");
        let writing = snapshot(&dir, &BTreeSet::new(), false, true);
        assert!(writing.parts.is_empty(), "正在写的那半截别报成垃圾");
        assert_eq!(writing.update.len(), 1, "定稿那一半与忙不忙无关");

        let r = clean_junk(&dir, &BTreeSet::new(), false);
        assert_eq!((r.items, r.bytes), (1, 6), "只收那半截，按字节报它自己的量");
        assert!(!d.join("pkg.zip.part1").exists());
        assert!(d.join("pkg.zip").exists(), "垃圾清理不碰已经落地的包");
        std::fs::remove_dir_all(dir).ok();
    }

    /// 「清过期」按版本目录里最新那份的 mtime；「清空全部」连新的一起收，
    /// 但**取件进行中一格都不动**
    #[test]
    fn cache_clean_takes_update_dirs_whole() {
        let dir = temp_cache("update-clean");
        let old = update_dir(&dir, "1.0.0", &[("a.zip", 5)], STALE_DAYS + 1);
        let fresh = update_dir(&dir, "1.1.0", &[("b.zip", 7)], 0);

        let r = clean_cache_with(&dir, CleanMode::Stale, false);
        assert_eq!((r.items, r.bytes), (1, 5));
        assert!(!old.exists());
        assert!(fresh.exists(), "刚下完的那版不该被「清过期」收走");

        let r = clean_cache_with(&dir, CleanMode::All, false);
        assert_eq!((r.items, r.bytes), (1, 7));
        assert!(!fresh.exists());

        let running = update_dir(&dir, "1.2.0", &[("c.zip", 9)], 0);
        let r = clean_cache_with(&dir, CleanMode::All, true);
        assert_eq!((r.items, r.bytes), (0, 0));
        assert!(running.exists(), "更新在跑时清理不许拆它的台");
        std::fs::remove_dir_all(dir).ok();
    }

    /// 只剩空壳的 `update\` 整条要收干净，否则设置页永远露着「1 个空目录」
    #[test]
    fn empty_update_tree_is_collected() {
        let dir = temp_cache("update-empty");
        let v = update_dir(&dir, "1.1.0", &[], 0);

        let u = usage(&dir, &BTreeSet::new(), false);
        assert_eq!((u.update_count, u.empty_dirs), (0, 2), "版本壳 + update 根壳各一项");

        assert_eq!(clean_junk(&dir, &BTreeSet::new(), false).items, 2);
        assert!(!v.exists());
        assert!(!dir.join(CACHE_UPDATE_DIR).exists());
        std::fs::remove_dir_all(dir).ok();
    }
}
