//! 24 小时那一趟自动检查的**账**：上次什么时候敲的、你看过没有、敲出来的结论是什么。
//!
//! 判据只在这个文件里，命令层只管「取本地事实 + 真去敲那一趟」：
//!
//! - 记的是**发起**时刻而不是成功时刻。失败的趟也算发过 ⇒ 一天顶多一次网络。匿名打 GitHub
//!   的 release 列表限流是 60/小时，每趟启动都敲的话，一台机器开十次应用就碰得上了。
//!   代价说清楚：断网时那一趟白跑，要等明天，或者用户自己点设置里那颗手动钮（它不走这道闸）。
//! - 结论**整份存下来**（不只是「有/没有」）：冷启动的角标要当场画得出，点开要有内容，
//!   不能为了亮一颗点先等一次网络。
//! - 读写都不报错。这本账坏了最贵的结果只是多敲一次网络，而一条没人点过的自动检查
//!   不该在界面上留下任何一句失败。

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::models::UpdateInfo;

/// 这本账的文件名。躺在**配置目录**而不是缓存：缓存那一档用户随时能点「清理缓存」，
/// 清掉时间戳等于「一清缓存就又开始每趟启动敲一次」，那道闸门就白搭了
pub const CHECK_FILE: &str = "update-check.json";

/// 两趟自动检查之间的最小间隔
pub const INTERVAL_MS: i64 = 24 * 60 * 60 * 1000;

/// 自动检查查出可更新的版本时发给前端的那一站（载荷就是这份 `UpdateInfo`）。
/// 前端收到只刷新角标，**不弹窗**——用户在忙别的的时候跳出一扇升级窗是打断
pub const EVENT_AVAILABLE: &str = "update://available";

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Stamp {
    /// 上次**发起**自动检查的时刻（UTC 毫秒）。0 = 从没查过
    pub at_ms: i64,
    /// 上次把这扇窗打开过的时刻（UTC 毫秒）。角标靠它跟自己比
    pub seen_ms: i64,
    /// 那一趟的结论。`None` = 那次连结论都没拿到（网络失败、解不懂）
    pub info: Option<UpdateInfo>,
}

impl Stamp {
    /// 该不该敲这一趟。没敲过、时钟被拨回去过、或已满 24 小时，都判「该敲」。
    ///
    /// `now < at_ms` 单独走「该敲」那一支：把系统时间往回拨一次，如果按差值算就会得到一个
    /// 负数——那等于**永久**不再检查，比多敲一次网络贵得多
    pub fn due(&self, now_ms: i64) -> bool {
        self.at_ms == 0 || now_ms < self.at_ms || now_ms - self.at_ms >= INTERVAL_MS
    }

    /// 角标该不该亮：那条 release 真能一键装，**且这次的结果你还没看过**。
    ///
    /// 「看过」就是 `seen_ms >= at_ms`：打开过一次弹窗之后角标自己灭，而明天那一趟把
    /// `at_ms` 往前挪，若仍是新版本就自己重新亮起来——不需要删文件、也不需要额外的标记
    pub fn badge(&self) -> bool {
        match &self.info {
            Some(info) => info.has_update && info.downloadable && self.seen_ms < self.at_ms,
            None => false,
        }
    }
}

pub fn path(config_dir: &Path) -> PathBuf {
    config_dir.join(CHECK_FILE)
}

/// 读那本账。读不到或读不懂 ⇒ 空账（`at_ms = 0` ⇒ 判「该敲」）
pub fn read(config_dir: &Path) -> Stamp {
    std::fs::read_to_string(path(config_dir))
        .ok()
        .and_then(|raw| serde_json::from_str::<Stamp>(&raw).ok())
        .unwrap_or_default()
}

/// 写那本账（best-effort，理由见文件头）。`sync_all` 与安装账本同一条规矩：
/// 自动检查敲完紧接着可能就是「点安装 → 进程被杀」那一拍，没落到盘上的时间戳等于明天再敲一次
pub fn write(config_dir: &Path, stamp: &Stamp) {
    if std::fs::create_dir_all(config_dir).is_err() {
        return;
    }
    let Ok(body) = serde_json::to_string(stamp) else {
        return;
    };
    let Ok(mut file) = File::create(path(config_dir)) else {
        return;
    };
    let _ = file
        .write_all(body.as_bytes())
        .and_then(|_| file.flush())
        .and_then(|_| file.sync_all());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stamp(at_ms: i64, seen_ms: i64) -> Stamp {
        Stamp {
            at_ms,
            seen_ms,
            info: None,
        }
    }

    #[test]
    fn a_day_is_exactly_the_gate() {
        let s = stamp(1_000, 0);
        // 差一小时没到 ⇒ 不敲
        assert!(!s.due(1_000 + INTERVAL_MS - 1));
        // 刚好满 24 小时 ⇒ 敲（边界归「该敲」那一侧，宁可多敲一次）
        assert!(s.due(1_000 + INTERVAL_MS));
        // 从没查过 ⇒ 敲
        assert!(stamp(0, 0).due(9_999));
    }

    #[test]
    fn a_clock_jumped_back_is_due() {
        // 拨回一小时：按差值算是负数，那样就永久不再检查了
        assert!(stamp(10 * INTERVAL_MS, 0).due(9 * INTERVAL_MS));
    }

    #[test]
    fn opening_the_dialog_is_what_clears_the_badge() {
        let mut s = stamp(5_000, 0);
        // 没有结论 ⇒ 永不亮
        assert!(!s.badge());

        s.info = Some(info(true, true));
        assert!(s.badge());
        // 看过恰好这一趟 ⇒ 灭
        s.seen_ms = 5_000;
        assert!(!s.badge());
        // 明天那一趟又敲出新版本 ⇒ 重新亮
        s.at_ms = 6_000;
        assert!(s.badge());
        // 有更新但装不了（缺签名、宿主不可信…）⇒ 不亮，那扇窗里才说得出原因
        s.info = Some(info(true, false));
        s.seen_ms = 0;
        assert!(!s.badge());
        // 已经是最新 ⇒ 不亮
        s.info = Some(info(false, true));
        assert!(!s.badge());
    }

    #[test]
    fn a_broken_book_reads_as_never_checked() {
        let dir = std::env::temp_dir().join(format!("ss-check-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        // 没这个文件
        assert_eq!(read(&dir).at_ms, 0);
        // 半截 JSON（上一次断电就停在这儿）
        std::fs::write(path(&dir), "{oops").unwrap();
        assert_eq!(read(&dir).at_ms, 0);
        assert!(read(&dir).due(1));

        write(&dir, &stamp(123, 45));
        let back = read(&dir);
        assert_eq!((back.at_ms, back.seen_ms), (123, 45));
        assert!(back.info.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 只填判定要看的那两格，其余给到能编译为止（这本账只认那两个 bool）
    fn info(has_update: bool, downloadable: bool) -> UpdateInfo {
        UpdateInfo {
            current: "1.0.0-beta.1".into(),
            latest: Some("1.0.0-beta.2".into()),
            tag: Some("v1.0.0-beta.2".into()),
            has_update,
            channel: crate::models::UpdateChannel::Beta,
            release_url: None,
            published_at: None,
            notes: None,
            assets: vec![],
            downloadable,
            blocked: None,
        }
    }
}
