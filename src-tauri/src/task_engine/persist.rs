//! 本地持久化：全局设置（settings.json）、任务存档（tasks.json）与转换模板（templates.json）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use tauri::{AppHandle, Manager};

use crate::core::ack::{ACK_FILE, SKIN_DIR, SKIN_FILE};
use crate::core::data_root::{self, APP_DATA_DIR_NAME, INSTALLER_FILE, SETTINGS_FILE};
use crate::models::*;
use super::events::trim_logs;
use super::state::Inner;
use super::util::now_ms;

/// 目录默认值迁移做过一次的凭据（空文件，与 settings.json 同目录）
const SETTINGS_MIGRATED: &str = "settings.migrated";
/// 任务本地存档：注册表全量快照（任务 + 方案 + 报告），重启后可见可重试
const TASKS_FILE: &str = "tasks.json";
/// 转换模板表（有序：数组下标就是转换页那颗下拉的顺序）
const TEMPLATES_FILE: &str = "templates.json";

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct TasksFile {
    #[serde(default)]
    tasks: HashMap<String, ConversionTask>,
    #[serde(default)]
    plans: HashMap<String, Vec<PlanMod>>,
    #[serde(default)]
    reports: HashMap<String, ConversionReport>,
}

/* ---------------- 配置目录 ---------------- */

/// 应用自己的状态落在哪，三档（顺序即优先级）：
/// 1. **便携包** → `{exe}\data`：整包搬走=连设置一起搬走，已定案，不参与后两档
/// 2. **安装版** → `{exe}\appdata`：用户挑了安装目录，设置就该落在那儿，卸载才清得掉
/// 3. **回落** → Tauri 的 app_config_dir（`%APPDATA%\{identifier}`）：开发构建（exe 躺在
///    `target\debug`，`cargo clean` 就把设置带走）和写不进去的位置（装进 Program Files 之类）
///
/// 收口成一个函数，是为了让「一切跟着 exe 走」这条语义只有一个实现点，不会漏掉某个文件。
/// 结果缓存在 OnceLock 里：判定含一次建目录 + 写探针，而这个函数在启动期被到处调
/// （settings、tasks、鸣谢快照、皮肤副本、WebView profile），每次真探一遍是白付的启动成本。
pub(crate) fn config_dir(app: &AppHandle) -> Option<PathBuf> {
    static CACHED: OnceLock<Option<PathBuf>> = OnceLock::new();
    CACHED
        .get_or_init(|| {
            let legacy = app.path().app_config_dir().ok();
            let exe_dir = std::env::current_exe().ok().and_then(|p| {
                p.parent().map(|d| d.to_path_buf())
            });
            let (to, from) = decide(
                data_root::portable_root().as_deref(),
                exe_dir.as_deref(),
                cfg!(debug_assertions),
                legacy.as_deref(),
            );
            // 搬不动就留在老目录：换到一个读不到原有设置的目录，用户看到的是「设置没了」，
            // 而这正是这次改动要避免的那种半途而废
            if let (Some(to), Some(from)) = (to.as_deref(), from) {
                if !migrate_from_legacy(to, &from) {
                    return Some(from);
                }
            }
            to
        })
        .clone()
}

/// 纯判定（参数化到能测）：返回 `(生效目录, 需要从哪儿搬)`，第二个只在第 2 档才可能是 Some。
/// 第 2 档要求 `{exe 同级}\appdata` **真写得进去**——只判 `exists()` 不够，只读介质上目录看着
/// 就在、写下去才知道不行。失败就静默回落老目录，不新增一个「这里不能写」的用户可见概念
fn decide(
    portable: Option<&Path>,
    exe_dir: Option<&Path>,
    dev: bool,
    legacy: Option<&Path>,
) -> (Option<PathBuf>, Option<PathBuf>) {
    if let Some(dir) = portable {
        return (Some(dir.to_path_buf()), None);
    }
    if !dev {
        if let Some(candidate) = exe_dir
            .map(|d| d.join(APP_DATA_DIR_NAME))
            .filter(|d| ensure_writable(d))
        {
            let from = legacy
                .filter(|l| l != &candidate.as_path())
                .map(|l| l.to_path_buf());
            return (Some(candidate), from);
        }
    }
    (legacy.map(|l| l.to_path_buf()), None)
}

/// 建目录（本来就要用）+ 写探针再删 ⇒ 确认这个目录能落文件
fn ensure_writable(dir: &Path) -> bool {
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    let probe = dir.join(".writable");
    let ok = std::fs::write(&probe, "").is_ok();
    if ok {
        let _ = std::fs::remove_file(probe);
    }
    ok
}

/// 配置目录里我们负责的文件。**列成清单而不是「搬整个目录」**：老目录里可能躺着
/// WebView2、插件或别人塞进来的东西，那些既不该由我们搬走，也不该由我们删
const KNOWN_CONFIG_ENTRIES: &[&str] = &[
    SETTINGS_FILE,
    SETTINGS_MIGRATED,
    TASKS_FILE,
    TEMPLATES_FILE,
    INSTALLER_FILE,
    ACK_FILE,
    SKIN_FILE,
    SKIN_DIR,
];

/// 「这份安装已经有主」的凭据：只有**应用自己**写过的文件算数。
/// `installer.json` 不能算——安装壳在应用第一次启动之前就先往新目录写它，把它当凭据
/// 会让升级用户的老设置永远搬不过来（判定当场成立，直接跳过搬运）。
/// `templates.json` 同理不算：它是那份设置搬过来之后才可能有的附件，顶上「有主」会把正主挡住
const OCCUPYING_CONFIG_ENTRIES: &[&str] = &[SETTINGS_FILE, SETTINGS_MIGRATED, TASKS_FILE];

/// 把老布局（`%APPDATA%\{identifier}`）里的已知文件**移动**到新配置目录。
/// 只在两处都成立时动手：目标里没有任何「有主」凭据（否则这是第二份安装，不能灌别人的数据），
/// 且源目录存在。搬完源目录空了就删，不空（躺着别人的文件）就留着。
///
/// 移动而不是复制：两份各自漂移比一次搬运贵得多。返回 `false` = 该搬的没搬成，
/// 调用方因此**不要**切到新目录（半途而废的迁移最查不出来）。
fn migrate_from_legacy(to: &Path, from: &Path) -> bool {
    if !from.is_dir() || OCCUPYING_CONFIG_ENTRIES.iter().any(|n| to.join(n).exists()) {
        return true;
    }
    let mut failed = false;
    for name in KNOWN_CONFIG_ENTRIES {
        let src = from.join(name);
        // 目标已经有同名文件（壳先写的 installer.json 就是这一类）：留住新那份，别拿老值顶掉
        if !src.exists() || to.join(name).exists() {
            continue;
        }
        if move_entry(&src, &to.join(name)).is_err() {
            failed = true;
        }
    }
    // 目录能空着删就删，删不掉（里面有不归我们的东西）就算了——留着不是错误
    let _ = std::fs::remove_dir(from);
    !failed
}

/// 移动一个条目：同盘 rename 一次就够；跨盘（设置在 C、装在 E）rename 会失败，
/// 这时退化成复制再删源。递归必须做，`skins\` 是个目录
fn move_entry(src: &Path, dst: &Path) -> std::io::Result<()> {
    if let Some(p) = dst.parent() {
        std::fs::create_dir_all(p)?;
    }
    if std::fs::rename(src, dst).is_ok() {
        return Ok(());
    }
    copy_entry(src, dst)?;
    remove_entry(src)
}

fn copy_entry(src: &Path, dst: &Path) -> std::io::Result<()> {
    if src.is_dir() {
        std::fs::create_dir_all(dst)?;
        for e in std::fs::read_dir(src)? {
            let e = e?;
            copy_entry(&e.path(), &dst.join(e.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(src, dst).map(|_| ())
    }
}

fn remove_entry(p: &Path) -> std::io::Result<()> {
    if p.is_dir() {
        std::fs::remove_dir_all(p)
    } else {
        std::fs::remove_file(p)
    }
}

/// 配置目录下的 WebView2 profile 目录名。放这里而不是 `data_root`：它不是数据根那一层，
/// 而且要跟着配置目录走——卸载清配置目录时才带得动它。
/// 里面 99% 是一次性的网络/JS 缓存（本机实测 532MB / 570MB），但也躺着主题那一行 localStorage
const WEBVIEW_PROFILE_DIR: &str = "webview";

/// WebView2 的数据目录（= 配置目录下的 `webview`）。建窗时交给它，见 `lib.rs` 的 setup：
/// 不给的话 Tauri 会强制 `%LOCALAPPDATA%\{identifier}`，那份几百 MB 谁都不会去清
pub(crate) fn webview_profile_dir(app: &AppHandle) -> Option<PathBuf> {
    Some(config_dir(app)?.join(WEBVIEW_PROFILE_DIR))
}

/* ---------------- 设置持久化 ---------------- */

fn settings_path(app: &AppHandle) -> Option<PathBuf> {
    config_dir(app).map(|d| d.join(SETTINGS_FILE))
}

/// 还没迁移 → 返回标记文件路径；已迁移或拿不到配置目录 → None（宁可不迁，也不能反复顶掉用户的路径）
fn migration_mark(app: &AppHandle) -> Option<PathBuf> {
    let mark = config_dir(app)?.join(SETTINGS_MIGRATED);
    (!mark.exists()).then_some(mark)
}

pub fn load_settings(app: &AppHandle) -> AppSettings {
    let home = app
        .path()
        .home_dir()
        .unwrap_or_else(|_| PathBuf::from("."));
    // 数据根：便携包 → exe 同级 data；安装版 → 安装壳指定的根；都没有才预选非系统盘
    let config = config_dir(app).unwrap_or_else(|| home.clone());
    let defaults = AppSettings::defaults_in(&data_root::suggested_root(&home, &config));
    let saved = settings_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str::<AppSettings>(&s).ok());
    match saved {
        Some(mut s) => {
            // 空目录字段回落默认值（用户清空输入框的场景）
            if s.output_dir.is_empty() {
                s.output_dir = defaults.output_dir.clone();
            }
            if s.cache_dir.is_empty() {
                s.cache_dir = defaults.cache_dir.clone();
            }
            s = s.normalized();

            // 一次性迁移：老版本的默认目录挂在用户目录下，那份绝对路径固化在 settings.json 里，
            // 不改写的话本次「预选非系统盘」对升级用户等于没发生。旧默认由同一套布局现算，
            // 不抄字符串（见 unstick_legacy 的取舍说明）。
            // 「已经迁过」必须靠标记文件记住，不能拿字段值当凭据：用户后来手动把目录设回
            // `~\SideShift\output` 时，那串字符和旧默认完全相同，没标记就会每次启动顶掉他一次
            if let Some(mark) = migration_mark(app) {
                let legacy = AppSettings::defaults_in(&home.join(data_root::DIR_NAME)).normalized();
                let out = unstick_legacy(&s.output_dir, &legacy.output_dir, &defaults.output_dir);
                let cache = unstick_legacy(&s.cache_dir, &legacy.cache_dir, &defaults.cache_dir);
                let changed = out != s.output_dir || cache != s.cache_dir;
                s.output_dir = out;
                s.cache_dir = cache;
                // 落盘成功才立标记：写失败就不立，下次启动重算——半途而废的迁移最查不出来
                if !changed || save_settings(app, &s).is_ok() {
                    let _ = std::fs::write(mark, "");
                }
            }
            s
        }
        None => defaults,
    }
}

/// 写 settings.json：失败要报给调用方。静默失败等于「用户改完设置、重启后回退」，
/// 而他看到的是保存成功的界面——这种账没法查，所以宁可吵一句。
pub fn save_settings(app: &AppHandle, s: &AppSettings) -> Result<(), String> {
    let Some(p) = settings_path(app) else {
        return Err("找不到应用配置目录，设置未能保存".to_string());
    };
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("设置写入失败：{e}（{}）", dir.display()))?;
    }
    let json = serde_json::to_string_pretty(s).map_err(|e| format!("设置序列化失败：{e}"))?;
    std::fs::write(&p, json).map_err(|e| format!("设置写入失败：{e}（{}）", p.display()))
}

/* ---------------- 转换模板 ---------------- */

fn templates_path(app: &AppHandle) -> Option<PathBuf> {
    config_dir(app).map(|d| d.join(TEMPLATES_FILE))
}

/// 启动回灌模板表。读不到/解析不了都当空表：模板是便利件，不是事实来源，
/// 为一个坏文件把设置与任务一起挡在门外不值。
pub fn load_templates(app: &AppHandle) -> Vec<ConversionTemplate> {
    let Some(Ok(text)) = templates_path(app).map(|f| std::fs::read_to_string(f)) else {
        return Vec::new();
    };
    // 裸数组：下标 = 转换页那颗下拉的顺序
    serde_json::from_str::<Vec<ConversionTemplate>>(&text).unwrap_or_default()
}

/// 整表写回：模板的唯一改动入口是「列表页拖完/编辑页保存」，两处都拿到全量，
/// 所以这里不做增量 upsert——一次写一个文件，顺序也就跟着一次落定。
pub fn save_templates(app: &AppHandle, list: &[ConversionTemplate]) -> Result<(), String> {
    let Some(p) = templates_path(app) else {
        return Err("找不到应用配置目录，模板未能保存".to_string());
    };
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("模板写入失败：{e}（{}）", dir.display()))?;
    }
    let json = serde_json::to_string_pretty(list).map_err(|e| format!("模板序列化失败：{e}"))?;
    std::fs::write(&p, json).map_err(|e| format!("模板写入失败：{e}（{}）", p.display()))
}

/* ---------------- 任务存档 ---------------- */

fn tasks_path(app: &AppHandle) -> Option<PathBuf> {
    config_dir(app).map(|d| d.join(TASKS_FILE))
}

/// 启动回灌：tasks/plans/reports 全量恢复；上次会话遗留的排队/运行中转会失败可重试
/// （流水线、源包解析缓存都不跨进程，复活即错）
pub fn load_tasks(app: &AppHandle, inner: &mut Inner) {
    let Some(Ok(text)) = tasks_path(app).map(|f| std::fs::read_to_string(f)) else { return };
    let Ok(v) = serde_json::from_str::<TasksFile>(&text) else { return };
    for (id, mut t) in v.tasks {
        if matches!(t.status, TaskStatus::Queued | TaskStatus::Running) {
            t.status = TaskStatus::Failed;
            t.error = Some(TaskError {
                stage: t.stage.unwrap_or(PipelineStage::Parser),
                title: "转换中断".into(),
                detail: "应用退出时任务尚未完成，可重试".into(),
                code: None,
                retryable: true,
                attempts: None,
                log_tail: None,
                exit_code: None,
            });
            t.finished_at = Some(now_ms());
        }
        // 旧版本没有日志上限，存档里可能躺着几千行（曾把界面卡死）——回灌时就裁掉
        trim_logs(&mut t.logs);
        inner.tasks.insert(id, t);
    }
    inner.plans = v.plans;
    inner.reports = v.reports;
}

/// 注册表任意变更后同步落盘（量小、低频，调用方持锁即可）
pub fn save_tasks(app: &AppHandle, inner: &Inner) {
    let Some(p) = tasks_path(app) else { return };
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let snapshot = TasksFile {
        tasks: inner.tasks.clone(),
        plans: inner.plans.clone(),
        reports: inner.reports.clone(),
    };
    if let Ok(json) = serde_json::to_string_pretty(&snapshot) {
        let _ = std::fs::write(p, json);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sideshift-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(dir: &Path, name: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "x").unwrap();
        p
    }

    /// 便携包优先于一切：它的数据本来就在 `{exe}\data`，没有「搬家」这件事
    #[test]
    fn portable_wins_and_never_migrates() {
        let portable = temp_dir("portable");
        let exe = temp_dir("portable-exe");
        let legacy = temp_dir("portable-legacy");
        assert_eq!(decide(Some(&portable), Some(&exe), false, Some(&legacy)), (Some(portable), None));
    }

    /// 开发构建一律留在 %APPDATA%：exe 躺在 `target\debug`，写在那儿等于 `cargo clean` 就丢设置
    #[test]
    fn dev_builds_stay_in_the_legacy_dir() {
        let exe = temp_dir("dev-exe");
        let legacy = temp_dir("dev-legacy");
        let (to, from) = decide(None, Some(&exe), true, Some(&legacy));
        assert_eq!(to, Some(legacy.clone()));
        assert_eq!(from, None, "留在老目录就没有搬家这回事");
        assert!(!exe.join(APP_DATA_DIR_NAME).exists(), "dev 不该在安装位置建目录");
    }

    /// 生产 + 可写的安装目录 ⇒ `{install}\appdata`，并记下要从老目录搬
    #[test]
    fn installed_builds_move_next_to_the_exe() {
        let exe = temp_dir("installed-exe");
        let legacy = temp_dir("installed-legacy");
        let (to, from) = decide(None, Some(&exe), false, Some(&legacy));
        assert_eq!(to, Some(exe.join(APP_DATA_DIR_NAME)));
        assert_eq!(from, Some(legacy));
    }

    /// 装进写不进去的位置（Program Files 之类）⇒ 静默回落，且不去搬：
    /// 半途而废的迁移比不换目录更难查
    #[test]
    fn unwritable_install_dir_falls_back_without_moving() {
        let blocker = touch(&temp_dir("unwritable"), "not-a-dir");
        let legacy = temp_dir("unwritable-legacy");
        let (to, from) = decide(None, Some(&blocker), false, Some(&legacy));
        assert_eq!(to, Some(legacy));
        assert_eq!(from, None);
    }

    /// 我们拥有的文件全搬走，别人的东西留下（所以老目录本身也留着）
    #[test]
    fn migration_moves_only_what_we_own() {
        let from = temp_dir("migrate-from");
        let to = temp_dir("migrate-to");
        touch(&from, SETTINGS_FILE);
        touch(&from, TASKS_FILE);
        touch(&from, &format!("{SKIN_DIR}/face.png"));
        touch(&from, "someone-else.log");
        assert!(migrate_from_legacy(&to, &from));
        assert!(to.join(SETTINGS_FILE).is_file());
        assert!(to.join(TASKS_FILE).is_file());
        assert!(to.join(SKIN_DIR).join("face.png").is_file());
        assert!(!from.join(SETTINGS_FILE).exists(), "移动不是复制，源不该留下");
        assert!(
            from.join("someone-else.log").is_file() && from.is_dir(),
            "不归我们的文件不能顺手删"
        );
    }

    /// 目标已经有数据 ⇒ 这是第二份安装，一份都不覆盖
    #[test]
    fn migration_never_overwrites_an_owned_target() {
        let from = temp_dir("occupy-from");
        let to = temp_dir("occupy-to");
        touch(&from, SETTINGS_FILE);
        std::fs::write(to.join(SETTINGS_FILE), b"mine").unwrap();
        assert!(migrate_from_legacy(&to, &from));
        assert_eq!(std::fs::read_to_string(from.join(SETTINGS_FILE)).unwrap(), "x");
        assert_eq!(std::fs::read_to_string(to.join(SETTINGS_FILE)).unwrap(), "mine");
    }

    /// 搬不动 ⇒ 报 false，调用方据此**不**切目录（宁可继续用老布局，也不能让人看见设置变默认值）
    #[test]
    fn failed_migration_aborts_the_switch() {
        let from = temp_dir("failing-from");
        let blocker = touch(&temp_dir("failing-to"), "not-a-dir");
        touch(&from, SETTINGS_FILE);
        assert!(!migrate_from_legacy(&blocker, &from));
        assert!(from.join(SETTINGS_FILE).is_file(), "搬不动就得原地不动");
    }

    /// 全新安装（老目录压根不存在）：什么都不做，也不能报错
    #[test]
    fn migration_without_a_source_is_a_no_op() {
        let to = temp_dir("fresh-to");
        let gone = std::env::temp_dir().join("sideshift-does-not-exist-anywhere");
        assert!(migrate_from_legacy(&to, &gone));
    }

    /// 跨盘那一退（rename 失败 → 复制再删）：拿「目标已存在的同名目录」逼出 rename 失败，
    /// 验的是退路本身，不是它在真跨盘上的表现
    #[test]
    fn move_entry_falls_back_to_copy_then_delete() {
        let root = temp_dir("copy-fallback");
        let src = root.join("src");
        std::fs::create_dir_all(src.join("nested")).unwrap();
        std::fs::write(src.join("nested/a.txt"), b"deep").unwrap();
        let dst = root.join("dst");
        std::fs::create_dir_all(&dst).unwrap();
        std::fs::write(dst.join("keep"), b"").unwrap();
        // 目录 → 非空目录：rename 必失败，退路必须把整棵树复制过去再删源
        assert!(move_entry(&src, &dst).is_ok());
        assert_eq!(std::fs::read(dst.join("nested/a.txt")).unwrap(), b"deep");
        assert!(!src.exists());
    }

    /// 安装壳赶在应用第一次启动之前往新目录写 `installer.json`。这份新文件既不能当
    /// 「已有主」的凭据（否则升级用户的老设置永远搬不过来），也不能被老目录那份顶掉
    #[test]
    fn shell_written_installer_json_neither_blocks_nor_gets_overwritten() {
        let from = temp_dir("installer-first-from");
        let to = temp_dir("installer-first-to");
        touch(&from, SETTINGS_FILE);
        std::fs::write(from.join(INSTALLER_FILE), b"stale").unwrap();
        std::fs::write(to.join(INSTALLER_FILE), b"fresh").unwrap();
        assert!(migrate_from_legacy(&to, &from));
        assert!(to.join(SETTINGS_FILE).is_file(), "老设置必须搬过来");
        assert_eq!(std::fs::read(to.join(INSTALLER_FILE)).unwrap(), b"fresh");
    }
}
