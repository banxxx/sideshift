//! 装：把已验签的那一包交给官方 NSIS 静默跑，成败靠**下次启动读盘对账**。
//!
//! 三条承重约束都落在这个文件，别处不再判一次：
//! 1. **账本必须在 spawn 之前落盘**。Windows 上拿不到安装器的回执：官方 NSIS 在 `/S` 下会先把
//!    正在运行的 `SideShift.exe` 直接杀掉（生成的安装脚本 `!insertmacro CheckIfAppIsRunning`，
//!    其定义在打包产物 `nsis/x64/utils.nsh:39-47` 的 `IfSilent → KillProcess` 那一支），
//!    连我们自己那句退出都可能等不到 ⇒ 唯一说得清「那次到底装成了没」的证据只能提前写死在盘上。
//! 2. **`/UPDATE` 必带，少了它会删用户数据**。生成的安装脚本 :314 在 update 档下整段跳过
//!    「先调起老卸载器」；不带它，升级就会跑老卸载器，而我们的钩子
//!    （`src-tauri/nsis/hooks.nsh:14`）在 `$UpdateMode <> 1` 时 `RMDir /r "$INSTDIR\appdata"`
//!    ——等于每次应用内升级把设置、任务存档、鸣谢快照与 WebView2 profile 清一遍。
//!    那道钩子只删 `$INSTDIR\appdata` 这一个子目录（文件头部专门解释了为什么绝不写
//!    `RMDir /r "$INSTDIR"`：安装目录可能与数据根是同一个），所以产物与缓存不在它射程里。
//! 3. **`/D=` 必须是最后一个参数**（NSIS 的写法约束：它取的是从 `/D=` 一直到行尾的那一段）。
//!    安装壳那边同样把它放在末尾，两处同序不是巧合。

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use super::fetch;
use crate::core::data_root;
use crate::core::downloader::app_code;
use crate::models::{UpdateOutcome, UpdateOutcomeKind};

/// 安装尝试那本账的文件名。**躺在配置目录而不是缓存**：缓存那一档用户随时能点「清理缓存」，
/// 账本被清掉等于那次安装没人认账——而配置目录在两个可清理类目之外
pub const JOURNAL_FILE: &str = "update-journal.json";

/// 一次安装尝试。这是**私有**的落盘格式：前端要的是「装成了没」那个结论
/// （`UpdateOutcome`），不是这几行字节——版本号对不对得上是 Rust 侧算的，判据不许有第二个实现点。
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
struct Journal {
    /// 那次试图装上去的版本（semver 串，不带 `v`）
    attempted: String,
    /// 按下那颗钮时本机是哪一版
    previous: String,
    /// 落盘时刻（UTC 毫秒）。界面上不说它，但真出事时「是哪一次」是唯一查得动的东西
    at_ms: i64,
}

fn journal_path(config_dir: &Path) -> PathBuf {
    config_dir.join(JOURNAL_FILE)
}

/// 落盘并 `sync_all`：这句之后我们那个进程随时可能被安装器杀掉，
/// 还留在 stdio 缓冲里的档在那一刻等于没写
fn write_journal(config_dir: &Path, j: &Journal) -> Result<(), String> {
    let err = || app_code("update-journal");
    std::fs::create_dir_all(config_dir).map_err(|_| err())?;
    let body = serde_json::to_string(j).map_err(|_| err())?;
    let mut file = File::create(journal_path(config_dir)).map_err(|_| err())?;
    file.write_all(body.as_bytes())
        .and_then(|_| file.flush())
        .map_err(|_| err())?;
    file.sync_all().map_err(|_| err())
}

/// 版本号对账：两边都去 `v` 再比。`attempted` 是我们自己写的 semver 串，
/// 但 `current` 那一份来自 `package_info()`，历史上两种写法都出现过，不该赌它
fn same_version(a: &str, b: &str) -> bool {
    a.trim_start_matches('v') == b.trim_start_matches('v')
}

/// 读并**收走**那份账（一次安装只报一次）。删在解析之前：解不开的档留着，下次启动还是解不开，
/// 却会每次都白问一遍
pub fn take_outcome(config_dir: &Path, current: &str) -> Option<UpdateOutcome> {
    let path = journal_path(config_dir);
    let raw = std::fs::read_to_string(&path).ok()?;
    let _ = std::fs::remove_file(&path);
    let j: Journal = serde_json::from_str(&raw).ok()?;
    Some(UpdateOutcome {
        kind: if same_version(&j.attempted, current) {
            UpdateOutcomeKind::Done
        } else {
            UpdateOutcomeKind::Unfinished
        },
        attempted: j.attempted,
        previous: j.previous,
    })
}

/// 交给 NSIS 的那串参数，顺序是契约而不是风格：`/D=` 必须收尾。
/// 带空格的目录交给 `Command` 去加引号——它按 Windows 的命令行规则包一整段，
/// 正是 NSIS 认的那个形状（`"/D=E:\My Programs\SideShift"`）
fn installer_args(install_dir: &Path) -> Vec<String> {
    vec![
        "/S".to_string(),
        "/UPDATE".to_string(),
        format!("/D={}", install_dir.display()),
    ]
}

/// 换文件那一跳：再验一遍盘上这一对 → 落账 → 静默跑安装器 → 请求退出。
///
/// 返回值只有失败那一路用得上：成功那一路之后这个进程很快就没了。
/// 装完不自动拉起新版——「点按钮 → 装 → 回来」是用户自己那两下，我们替他重启应用只会让人
/// 在没准备好的时候被拖回一个刚装完的窗口。
pub fn install(app: &AppHandle, config_dir: &Path, current: &str) -> Result<(), String> {
    // 便携形态没有「原目录」这层语义：NSIS 装进去的是一整套（注册表、快捷方式、卸载项），
    // 照这句装下去等于在别人的 U 盘里悄悄开一个安装版。便携要的是换掉裸 exe，那是另一条链
    if data_root::portable_root().is_some() {
        return Err(app_code("update-portable"));
    }
    // 开发构建（`pnpm tauri dev`，exe 躺在 `target\debug`）：这句会拿安装器往构建目录里装一整套，
    // 把上一次真构建的产物盖掉。这条链要测就测打包出来的那枚包
    if cfg!(debug_assertions) {
        return Err(app_code("update-dev-build"));
    }

    // 只认取件那一轮定稿的产物（不扫目录猜文件名：判据会和下载侧分叉）
    let (version, pkg_path) = fetch::ready_package().ok_or_else(|| app_code("update-not-ready"))?;
    let pkg_name = pkg_path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| app_code("update-name"))?;
    // 签名按「逐字同名 + .sig」找回，与下载侧 `Release::signature_for` 同一条规则
    let sig_path = pkg_path.with_file_name(format!("{pkg_name}.sig"));
    // 先问在不在，再验字节：中间隔的是「清理缓存」那一颗钮——它把这一对收走了，而内存里的
    // 状态还钉在 ready。只报一句「本机没能把安装包写下来」会把人引向磁盘，而真正要做的是重新下载
    if !pkg_path.is_file() || !sig_path.is_file() {
        return Err(app_code("update-not-ready"));
    }
    // spawn 前再验一遍：从「已就位」到「点了安装」之间隔的是用户的手，
    // 那段时间里盘上这两个字节属于本机任何进程，而一次读盘加验签是几十毫秒的量级
    fetch::landed_ok(&pkg_path, &sig_path)?;

    // 装回原目录：判据是「正在跑的这一个 exe 在哪儿」，比注册表那份 `InstallLocation` 硬——
    // 那就是本次要覆盖的目标本身，而注册表可能写着上一次装的位置
    let exe = std::env::current_exe().map_err(|_| app_code("update-path"))?;
    let install_dir = exe.parent().ok_or_else(|| app_code("update-path"))?;

    let journal = Journal {
        attempted: version,
        previous: current.to_string(),
        at_ms: chrono::Utc::now().timestamp_millis(),
    };
    // 这一句必须排在 spawn 之前，且失败就不 spawn：没有账本的安装，回来时说不清成败
    write_journal(config_dir, &journal)?;

    if std::process::Command::new(&pkg_path)
        .args(installer_args(install_dir))
        .spawn()
        .is_err()
    {
        // 安装器根本没起来，那句「没能开始安装」已经在界面上说出口了：
        // 账本留着只会在下次启动多报一句重复的「没装上」
        let _ = std::fs::remove_file(journal_path(config_dir));
        return Err(app_code("update-spawn"));
    }

    // 到这里这条链就没有我们的事了。`exit` 走的是 Tauri 的退出请求（顺带把插件与窗口的资源收掉），
    // 它异步；就算没等到，安装器那边也会把我们杀掉——那才是这条链的实际收场方式
    app.exit(0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sideshift-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// `/D=` 必须收尾，且 `/UPDATE` 与 `/S` 都在：少 `/UPDATE` 会删用户数据（见文件头第 2 条），
    /// 少 `/S` 会弹一个界面替用户决定什么时候退出
    #[test]
    fn installer_arguments_are_an_ordered_contract() {
        let args = installer_args(Path::new(r#"D:\My Programs\SideShift"#));
        assert_eq!(args[0], "/S");
        assert_eq!(args[1], "/UPDATE");
        assert_eq!(args[2], r#"/D=D:\My Programs\SideShift"#);
        assert_eq!(args.len(), 3, "多一颗就得回来看 /D= 还在不在末尾");
    }

    /// 账本写下去读得回，而且只报一次
    #[test]
    fn journal_survives_and_is_consumed() {
        let dir = temp_dir("journal");
        let j = Journal {
            attempted: "1.0.0-beta.4".into(),
            previous: "1.0.0-beta.3".into(),
            at_ms: 1,
        };
        write_journal(&dir, &j).unwrap();
        assert!(dir.join(JOURNAL_FILE).is_file(), "配置目录不存在时得先把目录建出来");

        let once = take_outcome(&dir, "1.0.0-beta.4").expect("刚写完必须读得回");
        assert_eq!(once.kind, UpdateOutcomeKind::Done);
        assert_eq!(once.previous, "1.0.0-beta.3");
        assert!(take_outcome(&dir, "1.0.0-beta.4").is_none(), "报过一次就该收走");
        std::fs::remove_dir_all(dir).ok();
    }

    /// 版本号对账三档：装成了 / 没装上 / `v` 前缀两边不一致。
    /// 中间那一档是这条链唯一能发现「安装器白跑一趟」的出口，判错方向就是骗用户
    #[test]
    fn outcome_reads_a_failed_update_as_unfinished() {
        let dir = temp_dir("journal-outcome");
        let j = Journal {
            attempted: "1.0.0-beta.4".into(),
            previous: "1.0.0-beta.3".into(),
            at_ms: 1,
        };

        write_journal(&dir, &j).unwrap();
        assert_eq!(
            take_outcome(&dir, "1.0.0-beta.3").unwrap().kind,
            UpdateOutcomeKind::Unfinished,
            "本机还是老版本 = 那次没装上"
        );
        write_journal(&dir, &j).unwrap();
        assert_eq!(
            take_outcome(&dir, "v1.0.0-beta.4").unwrap().kind,
            UpdateOutcomeKind::Done,
            "`v` 前缀不该把成功读成失败"
        );
        std::fs::remove_dir_all(dir).ok();
    }

    /// 坏账本（手改过、或写到一半断电）：回 `None` 且把文件收走，不能每次启动都问一遍
    #[test]
    fn broken_journal_is_taken_once_and_removed() {
        let dir = temp_dir("journal-broken");
        std::fs::write(dir.join(JOURNAL_FILE), "{ not json").unwrap();
        assert!(take_outcome(&dir, "1.0.0").is_none());
        assert!(
            !dir.join(JOURNAL_FILE).exists(),
            "解不开的档不该留在盘上等着被反复读"
        );
        std::fs::remove_dir_all(dir).ok();
    }
}
