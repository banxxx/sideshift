//! 取件与打包的实时账本：分目录日志聚合、联网/打包两侧的 activity 节流。
//!
//! 两条铁律：① 日志量级 = 前端性能预算——成百上千的离线条目必须攒成「一目录一条」；
//! ② 锁顺序固定「先本模块账本、后任务表」，任务表回调里不得回头锁账本（防互锁）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use tauri::AppHandle;

use crate::core::downloader::{Fetch, ItemSpec, TransferProgress};
use crate::models::{ActivityInfo, ActivityKind, ConversionTask, LogLevel, PipelineStage};
use super::events::log_update;
use super::state::AppState;
use super::util::fmt_size;

/// 离线取件的分组账本：整合包 / 本地 / 缓存条目成百上千，逐条打日志会同时打爆
/// 进度事件（前端每事件刷一次列表）与 tasks.json 落盘 —— 这正是"取件慢"的一半成因。
/// 口径改成一目录一条：按落位目录（模组 / 各保留目录）攒，取满该目录预设数量才出日志，
/// 取的过程中只按间隔发「不含日志」的进度心跳，保证进度条持续走动。
#[derive(Clone, Default)]
pub struct GroupTally {
    label: String,
    /// 本目录应取件总数，建取件计划时按与回调同一套路由口径算出
    expected: u32,
    files: u32,
    bytes: u64,
    pub done: usize,
    pub total: usize,
    flushed: bool,
}

impl GroupTally {
    fn new(label: String, expected: u32) -> Self {
        Self { label, expected, ..Default::default() }
    }
}

/// 分目录账本 + 进度心跳窗口（窗口起点记在结构体上，取走条目时不可连带清零）
#[derive(Default)]
pub struct FetchGroups {
    rows: Vec<GroupTally>,
    last_beat: Option<std::time::Instant>,
}

/// 距上次心跳超过这么久才推进度（日志仍只在目录取满时出）
const BEAT_INTERVAL: std::time::Duration = std::time::Duration::from_millis(250);

impl FetchGroups {
    pub fn new(plan: Vec<(String, u32)>) -> Self {
        Self {
            rows: plan
                .into_iter()
                .map(|(label, expected)| GroupTally::new(label, expected))
                .collect(),
            last_beat: None,
        }
    }

    /// 记一笔离线条目；返回 Some(快照) 表示该目录刚取满，应当出一条日志
    pub fn record(&mut self, label: &str, bytes: u64, done: usize, total: usize) -> Option<GroupTally> {
        if self.rows.iter().all(|r| r.label != label) {
            // 计划外的落位（理论上到不了这里）：按单条目成组，取到即算完成
            self.rows.push(GroupTally::new(label.to_string(), 1));
        }
        let r = self.rows.iter_mut().find(|r| r.label == label)?;
        r.files += 1;
        r.bytes += bytes;
        r.done = done;
        r.total = total;
        if r.flushed || r.files < r.expected {
            return None;
        }
        r.flushed = true;
        Some(r.clone())
    }

    /// 兜底：整轮取件结束后，仍有进账却没出过日志的目录各补一条（预设数对不上时不至于静默）
    pub fn pending(&mut self) -> Vec<GroupTally> {
        let mut out = Vec::new();
        for r in self.rows.iter_mut() {
            if !r.flushed && r.files > 0 {
                r.flushed = true;
                out.push(r.clone());
            }
        }
        out
    }

    /// 心跳是否该发，并在该发时重开窗口
    pub fn beat_due(&mut self) -> bool {
        let due = self
            .last_beat
            .map(|t| t.elapsed() >= BEAT_INTERVAL)
            .unwrap_or(true);
        if due {
            self.last_beat = Some(std::time::Instant::now());
        }
        due
    }
}

/// 分组完成日志（心跳日志同口径，只是不带完成结论）
pub fn group_line(g: &GroupTally) -> String {
    format!("取件 · {} · {} 个 · {}", g.label, g.files, fmt_size(g.bytes))
}

/// 该条目是否逐条上报。真正联网（未命中缓存）或重试过的才值得单列一条；
/// 其余（包内 / 本地 / 缓存命中）都是磁盘搬运，按落位目录攒成一条。
/// 建计划与取件回调共用此判定，两边口径不可能走偏。
pub fn reports_per_line(network: bool, cached: bool, retries: u32) -> bool {
    (network && !cached) || retries > 0
}

/// 分目录账本计划：每个落位目录预设取件多少条。
/// 「是否入组」直接走 reports_per_line，与取件回调同源，两边口径不会走偏。
pub fn group_plan_of(items: &[ItemSpec], staging: &Path, cached: &[bool]) -> Vec<(String, u32)> {
    let mut plan: Vec<(String, u32)> = Vec::new();
    for (it, c) in items.iter().zip(cached) {
        if reports_per_line(matches!(&it.fetch, Fetch::Url(_)), *c, 0) {
            continue;
        }
        let label = fetch_group_label(&it.dest, staging);
        match plan.iter_mut().find(|(l, _)| *l == label) {
            Some(g) => g.1 += 1,
            None => plan.push((label, 1)),
        }
    }
    plan
}

/// 落位路径 → 分组名：staging 下第一层目录就是一个取件目录（mods/ 说「模组」），
/// 根下散件只有加载器 jar
pub fn fetch_group_label(dest: &Path, staging: &Path) -> String {
    let rel = dest.strip_prefix(staging).unwrap_or(dest);
    let mut comps = rel.components();
    let Some(first) = comps.next() else {
        return "加载器".to_string();
    };
    if comps.next().is_none() {
        return "加载器".to_string();
    }
    let name = first.as_os_str().to_string_lossy().to_lowercase();
    if name == "mods" {
        "模组".to_string()
    } else {
        name
    }
}

pub fn apply_fetch_counts(
    t: &mut ConversionTask,
    done: usize,
    total: usize,
    net: &std::sync::atomic::AtomicU32,
    bytes: &std::sync::atomic::AtomicU64,
) {
    t.downloaded = Some(done as u32);
    t.total = Some(total as u32);
    t.net_done = Some(net.load(Ordering::Relaxed));
    t.done_bytes = Some(bytes.load(Ordering::Relaxed));
    t.progress = 30 + (52 * done as u32).checked_div(total as u32).unwrap_or(0).max(1);
}

/// 兜底刷出仍未出过日志的分组：整轮取件成功结束后走一次，账本对不上时不至于静默
pub fn flush_groups(
    app: &AppHandle,
    state: &Arc<AppState>,
    id: &str,
    groups: &Arc<Mutex<FetchGroups>>,
    net: &Arc<std::sync::atomic::AtomicU32>,
    bytes: &Arc<std::sync::atomic::AtomicU64>,
) {
    for g in groups.lock().unwrap().pending() {
        let (net_u, bytes_u) = (net.clone(), bytes.clone());
        log_update(
            app,
            state,
            id,
            PipelineStage::Downloader,
            LogLevel::Info,
            &group_line(&g),
            move |t| apply_fetch_counts(t, g.done, g.total, &net_u, &bytes_u),
        );
    }
}

/* ---------------- 当前动作（实时条，不进日志环） ---------------- */

/// 实时条最小重绘间隔：进度事件一次要全量刷 UI，逐块发会把界面烧穿
/// （日志量级 = 前端性能预算，同理适用于 activity）
const ACTIVITY_WINDOW: std::time::Duration = std::time::Duration::from_millis(150);

/// 联网传输的实时账本：reqwest 每个响应块回调一次，比日志密两个数量级，
/// 这里只累加字节，到窗口点才产出一条 ActivityInfo 随进度事件下发。
/// 锁顺序固定为「先本账本、后任务表」，且任务表回调里不得回头锁账本（防互锁）。
#[derive(Default)]
pub struct NetActivity {
    /// dest → (文件名, 已收, 该响应 Content-Length)：只装进行中的条目，收完即结算移出
    pub inflight: HashMap<PathBuf, (String, u64, u64)>,
    /// 已完成联网条目的字节
    settled: u64,
    /// 已完成条目里 Content-Length 已知的那部分（计划没给量时用它兜底）
    settled_known: u64,
    /// 计划口径：需联网总字节 / 总条数（建计划时算好，整轮不变）
    planned_bytes: u64,
    items_total: u32,
    subject: String,
    attempt: u32,
    last_emit: Option<std::time::Instant>,
    last_bytes: u64,
    rate: f64,
}

impl NetActivity {
    pub fn new(planned_bytes: u64, items_total: u32) -> Self {
        Self { planned_bytes, items_total, ..Default::default() }
    }

    fn done(&self) -> u64 {
        self.settled + self.inflight.values().map(|(_, d, _)| *d).sum::<u64>()
    }

    /// 分母优先取计划量（含还没开跑的条目），计划没给数才退回 Content-Length 累加
    fn total(&self) -> u64 {
        if self.planned_bytes > 0 {
            return self.planned_bytes;
        }
        self.settled_known + self.inflight.values().map(|(_, _, t)| *t).sum::<u64>()
    }

    pub fn snapshot(&self, items_done: u32) -> ActivityInfo {
        ActivityInfo {
            kind: ActivityKind::Net,
            subject: self.subject.clone(),
            done_bytes: self.done(),
            total_bytes: self.total(),
            items_done,
            items_total: self.items_total,
            rate_bps: self.rate,
            attempt: self.attempt.max(1),
        }
    }

    /// 记一个响应块；返回 Some(info) 表示到出图点了。速率按两次出图之间的字节差算，
    /// 再做一点平滑，免得采样窗口边界上数字乱跳
    pub fn record(&mut self, p: &TransferProgress, items_done: u32) -> Option<ActivityInfo> {
        let e = self
            .inflight
            .entry(p.key.clone())
            .or_insert_with(|| (p.file_name.clone(), 0, 0));
        e.1 = p.done;
        e.2 = p.total;
        self.subject = p.file_name.clone();
        self.attempt = p.attempt;
        let now = std::time::Instant::now();
        let due = self
            .last_emit
            .map(|t| now.duration_since(t) >= ACTIVITY_WINDOW)
            .unwrap_or(true);
        let done = self.done();
        if let Some(prev) = self.last_emit {
            let dt = now.duration_since(prev).as_secs_f64();
            if dt > 0.0 {
                let inst = done.saturating_sub(self.last_bytes) as f64 / dt;
                self.rate = if self.rate > 0.0 { self.rate * 0.7 + inst * 0.3 } else { inst };
            }
        }
        if !due {
            return None;
        }
        self.last_emit = Some(now);
        self.last_bytes = done;
        Some(self.snapshot(items_done))
    }

    /// 一条收完：从在飞集合结算。集合里没有的说明是缓存命中（零传输），不计
    pub fn settle(&mut self, key: &Path, bytes: u64) {
        if let Some((_, _, known)) = self.inflight.remove(key) {
            self.settled += bytes;
            self.settled_known += known;
        }
    }
}

/// 打包实时账本：ZipWriter 每写一个文件回调一次，口径与联网侧一致
#[derive(Default)]
pub struct ZipActivity {
    files_total: u32,
    files_done: u32,
    bytes_total: u64,
    bytes_done: u64,
    subject: String,
    started: Option<std::time::Instant>,
    last_emit: Option<std::time::Instant>,
}

impl ZipActivity {
    pub fn plan(&mut self, files: usize, bytes: u64) {
        self.files_total = files as u32;
        self.bytes_total = bytes;
        self.started = Some(std::time::Instant::now());
    }

    pub fn snapshot(&self) -> ActivityInfo {
        let secs = self.started.map(|s| s.elapsed().as_secs_f64()).unwrap_or(0.0);
        ActivityInfo {
            kind: ActivityKind::Zip,
            subject: self.subject.clone(),
            done_bytes: self.bytes_done,
            total_bytes: self.bytes_total,
            items_done: self.files_done,
            items_total: self.files_total,
            // 打包侧看的是平均吞吐：单文件之间的瞬时差没有意义
            rate_bps: if secs > 0.0 { self.bytes_done as f64 / secs } else { 0.0 },
            attempt: 1,
        }
    }

    pub fn file(&mut self, group: &str, bytes: u64) -> Option<ActivityInfo> {
        self.files_done += 1;
        self.bytes_done += bytes;
        self.subject = group.to_string();
        let now = std::time::Instant::now();
        let due = self
            .last_emit
            .map(|t| now.duration_since(t) >= ACTIVITY_WINDOW)
            .unwrap_or(true);
        if !due {
            return None;
        }
        self.last_emit = Some(now);
        Some(self.snapshot())
    }
}

/// 打包进度：84 → 99 按已写字节铺开。旧写法全程钉在 90 再跳 100，几百个文件的
/// 压缩时间里界面一动不动，看起来就像卡死
pub fn build_progress(done: u64, total: u64) -> u32 {
    let step = done.saturating_mul(15).checked_div(total.max(1)).unwrap_or(0).min(15);
    84 + step as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// 分组名口径：staging 第一层目录即一条日志的归属，mods/ 说「模组」，根下散件说「加载器」
    #[test]
    fn fetch_group_label_maps_dest_to_folder() {
        let staging = PathBuf::from("/cache/tasks/t1/staging");
        assert_eq!(fetch_group_label(&staging.join("mods").join("a.jar"), &staging), "模组");
        assert_eq!(fetch_group_label(&staging.join("config").join("a.toml"), &staging), "config");
        assert_eq!(
            fetch_group_label(&staging.join("kubejs").join("client").join("x.js"), &staging),
            "kubejs"
        );
        assert_eq!(fetch_group_label(&staging.join("fabric-server-launch.jar"), &staging), "加载器");
        // 大小写不同的 Mods/ 归同一组
        assert_eq!(fetch_group_label(&staging.join("Mods").join("a.jar"), &staging), "模组");
    }

    /// 只有「真联网」的条目逐条报：命中缓存的联网坐标、包内、本地条目一律归入目录账本
    #[test]
    fn only_real_network_reports_per_line() {
        assert!(reports_per_line(true, false, 0), "未命中缓存的联网条目逐条报");
        assert!(!reports_per_line(true, true, 0), "联网坐标命中缓存 → 走复制，入目录");
        assert!(!reports_per_line(false, false, 0), "包内/本地 → 入目录");
        assert!(reports_per_line(false, false, 2), "重试过就该看得见");
    }

    fn spec_at(fetch: Fetch, dest: PathBuf) -> ItemSpec {
        ItemSpec {
            file_name: dest
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default(),
            sha1: None,
            dest,
            size_bytes: 10,
            fetch,
        }
    }

    /// 计划与回调同源：按 group_plan_of 的预设数走一遍回调路由，每个目录恰好出一条
    #[test]
    fn group_plan_matches_runtime_routing() {
        let staging = PathBuf::from("/cache/tasks/t1/staging");
        let archive = PathBuf::from("/pack.mrpack");
        let items = vec![
            spec_at(Fetch::ZipEntry { archive: archive.clone(), entry: "override/config/a.cfg".into() }, staging.join("config").join("a.cfg")),
            spec_at(Fetch::ZipEntry { archive: archive.clone(), entry: "override/config/b.cfg".into() }, staging.join("config").join("b.cfg")),
            spec_at(Fetch::Local(PathBuf::from("/local/a.jar")), staging.join("mods").join("a.jar")),
            spec_at(Fetch::Url("https://m/1.jar".into()), staging.join("mods").join("1.jar")),
            spec_at(Fetch::Url("https://m/2.jar".into()), staging.join("mods").join("2.jar")),
            spec_at(Fetch::Url("https://dl/fabric.jar".into()), staging.join("fabric-server-launch.jar")),
        ];
        // 联网坐标里 mods/2.jar 与根下的服务端 jar 命中缓存 → 走复制，归入各自目录
        let cached = [false, false, false, false, true, true];
        let plan = group_plan_of(&items, &staging, &cached);
        assert_eq!(
            plan,
            vec![("config".to_string(), 2u32), ("模组".to_string(), 2u32), ("加载器".to_string(), 1u32)]
        );

        let mut g = FetchGroups::new(plan);
        let mut per_line = 0;
        let mut group_lines = 0;
        for (i, it) in items.iter().enumerate() {
            let network = matches!(&it.fetch, Fetch::Url(_));
            if reports_per_line(network, cached[i], 0) {
                per_line += 1;
                continue;
            }
            let label = fetch_group_label(&it.dest, &staging);
            if g.record(&label, 10, i + 1, items.len()).is_some() {
                group_lines += 1;
            }
        }
        assert_eq!(per_line, 1, "只有未命中缓存的那个联网 jar 逐条报");
        assert_eq!(group_lines, 3, "三个目录各一条");
        assert!(g.pending().is_empty(), "计划与回调对得上时不该有兜底残留");
    }

    /// 一个目录只出一条：取满预设数才发，过程中只走心跳
    #[test]
    fn groups_emit_one_line_per_folder_when_full() {
        let mut g = FetchGroups::new(vec![("模组".into(), 2), ("config".into(), 3)]);
        assert!(g.record("模组", 10, 1, 5).is_none());
        let snap = g.record("模组", 20, 2, 5).expect("取满 2 个应出一条");
        assert_eq!(snap.files, 2);
        assert_eq!(snap.bytes, 30);
        assert_eq!(group_line(&snap), "取件 · 模组 · 2 个 · 30 B");
        // 同目录再来一条（预设外的迟到项）不应重复出
        assert!(g.record("模组", 99, 3, 5).is_none());
        // 未取满的目录不出
        assert!(g.record("config", 1, 4, 5).is_none());
        assert!(g.record("config", 1, 5, 5).is_none());
        assert!(g.record("config", 1, 6, 5).is_some());
        // 兜底只补没出过的，且已出过的不再重复
        let left = g.pending();
        assert!(left.iter().all(|r| r.label != "模组"));

        let mut g2 = FetchGroups::new(vec![("kubejs".into(), 100)]);
        assert!(g2.record("kubejs", 5, 1, 100).is_none());
        let left = g2.pending();
        assert_eq!(left.len(), 1, "未取满的目录在整轮结束后要兜底补一条");
        assert_eq!(left[0].files, 1);
        assert!(g2.pending().is_empty(), "兜底刷出后不应重复");
    }

    /// 心跳：首次立即可发（进度条别等到取满才动），随后受间隔约束；
    /// 窗口起点记在账本上，取条目/发日志都不该把它清空
    #[test]
    fn group_beat_throttles_progress_only_updates() {
        let mut g = FetchGroups::new(vec![("config".into(), 9999)]);
        assert!(g.beat_due(), "首次应立即可发，否则进度条会长时间不动");
        assert!(!g.beat_due(), "间隔未到不应再发");
        g.last_beat = Some(std::time::Instant::now() - BEAT_INTERVAL * 2);
        assert!(g.beat_due(), "间隔已过应再发");
    }

    fn tp(key: &str, done: u64, total: u64, attempt: u32) -> TransferProgress {
        TransferProgress {
            file_name: format!("{key}.jar"),
            key: PathBuf::from(key),
            done,
            total,
            attempt,
        }
    }

    /// 实时条：并发下载的字节合到一条、窗口未到不出图、结算不重复计数
    #[test]
    fn net_activity_aggregates_concurrent_files_and_throttles() {
        let mut a = NetActivity::new(300, 2);
        assert!(a.record(&tp("/d/a", 50, 150, 1), 0).is_some(), "首次应立即出图，否则条不动");
        assert!(a.record(&tp("/d/b", 30, 150, 1), 0).is_none(), "窗口未到不应再出图");
        assert_eq!(a.done(), 80, "在飞两条的字节应合并计数");
        a.settle(&PathBuf::from("/d/a"), 150);
        assert_eq!(a.done(), 180, "结算按实际字节计，不与在飞量重复");
        assert_eq!(a.total(), 300, "计划有量就用计划量当分母");
        a.last_emit = Some(std::time::Instant::now() - ACTIVITY_WINDOW * 2);
        let info = a.record(&tp("/d/b", 150, 150, 2), 1).expect("窗口已过应再出图");
        assert_eq!(info.done_bytes, 300);
        assert_eq!(info.attempt, 2, "第几次重试要看得见");
        assert_eq!(info.items_total, 2);
        assert!(info.rate_bps > 0.0, "速率应算出来");
    }

    /// 计划没给字节（例如 maven 坐标无伴生大小）：分母退回 Content-Length 累加
    #[test]
    fn net_activity_falls_back_to_content_length() {
        let mut a = NetActivity::new(0, 2);
        a.record(&tp("/d/a", 10, 100, 1), 0).unwrap();
        a.settle(&PathBuf::from("/d/a"), 10);
        a.last_emit = Some(std::time::Instant::now() - ACTIVITY_WINDOW * 2);
        let info = a.record(&tp("/d/b", 5, 50, 1), 1).expect("窗口已过应出图");
        assert_eq!(info.total_bytes, 150, "已结算 + 在飞的 Content-Length");
        assert_eq!(info.done_bytes, 15);
    }

    /// 打包侧同样按窗口出图，subject 跟随当前顶层目录
    #[test]
    fn zip_activity_throttles_per_file() {
        let mut z = ZipActivity::default();
        z.plan(3, 300);
        assert!(z.file("模组", 100).is_some(), "首个文件应立即出图");
        assert!(z.file("模组", 100).is_none(), "窗口未到不应再出图");
        z.last_emit = Some(std::time::Instant::now() - ACTIVITY_WINDOW * 2);
        let info = z.file("根文件", 100).expect("窗口已过应出图");
        assert_eq!((info.done_bytes, info.total_bytes), (300, 300));
        assert_eq!((info.items_done, info.items_total), (3, 3));
        assert_eq!(info.subject, "根文件");
        assert_eq!(info.kind, ActivityKind::Zip);
    }

    /// 打包进度必须在 84→99 之间真实铺开（旧写法全程钉 90，看着像卡死）
    #[test]
    fn build_progress_spreads_over_the_zip_stage() {
        assert_eq!(build_progress(0, 1000), 84);
        assert_eq!(build_progress(500, 1000), 91);
        assert_eq!(build_progress(1000, 1000), 99, "打包阶段不满 100，成功收尾才给 100");
        assert_eq!(build_progress(0, 0), 84, "总量为 0 不能崩");
    }
}
