//! 卸载壳的投递与注册表入口维护：安装壳（首装）与主应用（更新后自愈）两处消费，
//! 用 `#[path]` 编进各自 crate（与 `data_root.rs` 同一条源码级复用路线）。
//!
//! 两处消费的差异只有三件：壳字节来自**各自的 build.rs 内嵌**（各自的 OUT_DIR）、
//! 什么时候跑（安装壳在装完那一刻、主应用在每次启动自愈）、入口判定的宽严
//! （两者都验目录归属，见 `classify_entry`）。其余行为逐字一致：**改一处必改另一处**。
//!
//! 「投递失败只有一句 Note、不打日志也不弹窗」是刻意的设计而非疏漏：Note 的产生场景
//! 在构建端已被消灭（release 构建缺壳由 build.rs 硬失败，见主应用 build.rs），剩下的
//! 只有运行期罕见的注册表/IO 失败——每一种的后果都是「退回原生卸载器，控制面板照常
//! 可卸」，功能上没有损失；release 是 windows 子系统没有控制台，为一种自愈的降级态
//! 专门起一条 UI 通知，与它「用户没做错任何事」的性质不匹配。

use std::path::Path;

#[allow(dead_code)]
#[path = "data_root.rs"]
mod data_root;

/// 三个文件名的打包引用。真源是 `data_root` 的同名常量（且与 `nsis/hooks.nsh` 的字面量联动），
/// 这里嵌一份只含引用形状的表，避免消费方各自散写
pub const NAMES: Names = Names {
    shell: data_root::UNINSTALL_SHELL_NAME,
    native: data_root::NSIS_UNINSTALLER_NAME,
    stashed: data_root::NSIS_UNINSTALLER_STASHED,
};

/// 三个文件名。真源见 `data_root` 的同名常量
#[derive(Clone, Copy, Debug)]
pub struct Names {
    /// 自绘卸载壳（投递目标）
    pub shell: &'static str,
    /// NSIS 原生卸载器（收存前在安装目录里的名字）
    pub native: &'static str,
    /// 原生卸载器收进配置目录之后的名字
    pub stashed: &'static str,
}

/// 壳字节（各消费方的 build.rs 写进各自的 OUT_DIR；空 = 这枚二进制没内嵌，自愈安静跳过）
const SHELL_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/uninstall-shell.bin"));

/// 卸载入口现在指向谁
pub enum Entry {
    /// 原生入口 → 改成壳
    Rewrite(String),
    /// 已经是壳 → 什么都不动
    Already,
    /// 空 / 读不到 / 不是我们的卸载器 → 不碰
    Foreign,
}

/// 投递的结论
pub enum Deliver {
    /// 入口已经指向壳，文件也在——什么都不缺
    Already,
    /// 投了壳、入口改指成功、原生卸载器已收存——三件事全部落地
    Retargeted,
    /// 没换成或没动（说明性话术，应用/安装本身照旧可用：原生卸载器还在，控制面板卸得掉）
    Note(String),
}

/// 主应用启动时的自愈入口（安装壳不走这里，它直接调 `deliver`）。
///
/// `exe_dir` = 当前 exe 所在目录（安装版的安装目录），`config_dir` = 配置目录（原生卸载器的收存处）。
/// 两道前置闸都在这里而不是调用方：**dev 构建**整个自愈不跑（dev 的 exe 在 target 里，
/// 就算目录守卫拦得住注册表改写，「每次启动白读一次键」也是噪声）；**可攜形态**不跑
/// （没有安装器写的注册表键，那套「控制面板卸载」的语义在可攜版不存在）。
/// `tauri` 的路径解析不进本模块——它要被编进安装壳，那边的 AppHandle 是另一个上下文
pub fn self_heal(exe_dir: &Path, config_dir: &Path) -> Option<String> {
    if cfg!(debug_assertions) || data_root::portable_root().is_some() {
        return None;
    }
    match deliver(&NAMES, exe_dir, config_dir) {
        Deliver::Already | Deliver::Retargeted => None,
        Deliver::Note(m) => Some(m),
    }
}

/// 入口判定。`own_dir` = 当前 exe 所在的目录，**必传**：两个消费方（安装壳的首装投递、
/// 主应用的启动自愈）面对的键都可能来自任何地方——安装壳那边 NSIS 刚写的键虽然路径必然
/// 是本次安装目录，但「文件名 + 目录」双判据对它同样成立，没有为它省掉目录的必要。
/// 入口指着的卸载器必须就在 `own_dir` 里才动手：dev 进程、可攜形态、别的安装，都自然跳过。
///
/// 已经指向壳（哪怕在别的目录）永远判 `Already` 不改写：Already 不写任何东西，是安全的；
/// 真正的改写只发生在原生入口上，而那一条被目录守卫拦住。
pub fn classify_entry(
    existing: Option<&str>,
    names: &Names,
    install_dir: &Path,
    own_dir: &Path,
) -> Entry {
    let Some(raw) = existing.map(str::trim).filter(|s| !s.is_empty()) else {
        return Entry::Foreign;
    };
    // 模板写进去的值是带引号的 `"$INSTDIR\uninstall.exe"`，读回来原样带着那对引号
    let unquoted = raw.trim_matches('"');
    let Some(name) = Path::new(unquoted).file_name().and_then(|s| s.to_str()) else {
        return Entry::Foreign;
    };
    let name = name.to_ascii_lowercase();
    if name == names.shell.to_ascii_lowercase() {
        return Entry::Already;
    }
    if name != names.native.to_ascii_lowercase() {
        return Entry::Foreign;
    }
    let Some(entry_dir) = Path::new(unquoted).parent() else {
        return Entry::Foreign;
    };
    if !same_dir(entry_dir, own_dir) {
        return Entry::Foreign;
    }
    Entry::Rewrite(format!("\"{}\"", install_dir.join(names.shell).display()))
}

/// 目录等价：能 canonicalize 就比规范化结果（吃掉 8.3 短名与大小写），
/// 不能就退到「分隔符统一 + 小写」的宽松比对
fn same_dir(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a.as_os_str().eq_ignore_ascii_case(b.as_os_str()),
        _ => {
            let norm = |p: &Path| p.to_string_lossy().replace('/', "\\").to_ascii_lowercase();
            norm(a) == norm(b)
        }
    }
}

/// 投递卸载壳，并（入口改指成功时）把 NSIS 重新生成的那份原生卸载器收进配置目录。
///
/// 入口已是壳且文件还在 ⇒ 什么都不做（每次启动重写几 MB 是无谓的磁盘折腾）；
/// 入口是原生卸载器 ⇒ 投壳 + 改指 + 收存——这正是「应用内更新的静默安装重写了入口」的
/// 自愈路径；入口是别的东西 ⇒ 一字不改。壳字节为空（这一枚二进制没内嵌）⇒ 安静跳过。
pub fn deliver(names: &Names, install_dir: &Path, config_dir: &Path) -> Deliver {
    if SHELL_BYTES.is_empty() {
        return Deliver::Note("这一枚构建没有内嵌卸载壳，自愈跳过".into());
    }
    let target = install_dir.join(names.shell);
    let Ok(existing) = read_uninstall_string() else {
        return Deliver::Note(
            "卸载界面没换成自带的那套：读卸载注册表键失败。控制面板里的卸载仍然可用".into(),
        );
    };
    match classify_entry(existing.as_deref(), names, install_dir, install_dir) {
        Entry::Already if target.is_file() => Deliver::Already,
        // 入口已是壳但文件不在（被人动了）：补投一份，注册表不用改
        Entry::Already => match write_shell(&target) {
            Ok(()) => Deliver::Already,
            Err(e) => Deliver::Note(format!(
                "卸载界面没换成自带的那套：{e}。控制面板里的卸载仍然可用"
            )),
        },
        Entry::Rewrite(value) => {
            if let Err(e) = write_shell(&target) {
                return Deliver::Note(format!(
                    "卸载界面没换成自带的那套：{e}。控制面板里的卸载仍然可用"
                ));
            }
            if let Err(e) = write_uninstall_string(&value) {
                return Deliver::Note(format!(
                    "卸载界面没换成自带的那套：{e}。控制面板里的卸载仍然可用"
                ));
            }
            // 主 NSIS 包（应用内更新的静默安装）刚重新生成过原生卸载器：收进配置目录，
            // 让壳之后调起的永远是当前版本的那一份
            stash(names, install_dir, config_dir);
            Deliver::Retargeted
        }
        Entry::Foreign => Deliver::Note(
            "卸载入口不是这个安装目录写的，没有去改它。控制面板里的卸载仍然可用".into(),
        ),
    }
}

fn write_shell(target: &Path) -> Result<(), String> {
    std::fs::write(target, SHELL_BYTES).map_err(|e| format!("写 {} 失败（{e}）", target.display()))
}

/// 把 NSIS 那份原生卸载器收进配置目录。安装目录里躺着两个能卸载东西的 exe，人和杀毒软件都分不清
/// 哪个是正主；而它又**不能删**——快捷方式（含从任务栏取消固定）、注册表项、文件清单都写在它的
/// 删除清单里，卸载壳只是把界面换成我们这套，真正动手的仍是它。所以是"收起来"，不是"删掉"。
///
/// 挪不动就算了（目标被占用/跨卷）：卸载壳两处都认，最坏的后果是那个文件还看得见，不是卸不掉。
/// Windows 上 `rename` 会覆盖同名目标，所以升级时上一份留在那儿也不用先删——先删再挪失败就等于
/// 把唯一的原生入口弄没了
fn stash(names: &Names, install_dir: &Path, config_dir: &Path) {
    let from = install_dir.join(names.native);
    if from.is_file() {
        let _ = std::fs::create_dir_all(config_dir);
        let _ = std::fs::rename(&from, config_dir.join(names.stashed));
    }
}

/// `UNINSTKEY` = `Software\Microsoft\Windows\CurrentVersion\Uninstall\SideShift`
/// （生成脚本 :61）。产品名取自 src-tauri/tauri.conf.json，改那一边要同步这一边——
/// 不同步的后果是"谁都改不到"，卸载入口保持原生，属于安全的那一侧
#[cfg(windows)]
const UNINST_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\SideShift";

/// 读注册表里的 UninstallString。键/值不存在 ⇒ `Ok(None)`（不是错误：那说明
/// 「这个键不是我们这条链写的」，判定交给调用方）
#[cfg(windows)]
fn read_uninstall_string() -> Result<Option<String>, String> {
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ, REG_SZ,
    };

    let key = wide(UNINST_KEY);
    let name = wide("UninstallString");
    let mut h: HKEY = std::ptr::null_mut();
    unsafe {
        let opened = RegOpenKeyExW(HKEY_CURRENT_USER, key.as_ptr(), 0, KEY_READ, &mut h);
        if opened == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if opened != ERROR_SUCCESS {
            return Err(format!("打开卸载注册表键失败（{opened}）"));
        }

        // 先量长度再读：RegQueryValueExW 的两段式调用没有别的写法
        let mut len = 0u32;
        let mut kind = 0u32;
        let probe = RegQueryValueExW(
            h,
            name.as_ptr(),
            std::ptr::null(),
            &mut kind,
            std::ptr::null_mut(),
            &mut len,
        );
        let mut existing = None;
        if probe == ERROR_SUCCESS && len > 0 && kind == REG_SZ {
            let mut buf = vec![0u16; len as usize / 2 + 1];
            let mut got = len;
            let read = RegQueryValueExW(
                h,
                name.as_ptr(),
                std::ptr::null(),
                &mut kind,
                buf.as_mut_ptr() as *mut u8,
                &mut got,
            );
            if read == ERROR_SUCCESS {
                let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
                existing = Some(String::from_utf16_lossy(&buf[..end]));
            }
        }
        RegCloseKey(h);
        Ok(existing)
    }
}

/// 把 UninstallString 改写成 `value`（带引号、含结尾 NUL，与 NSIS 自己写这条值时一致）
#[cfg(windows)]
fn write_uninstall_string(value: &str) -> Result<(), String> {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ,
    };

    let key = wide(UNINST_KEY);
    let name = wide("UninstallString");
    let mut h: HKEY = std::ptr::null_mut();
    unsafe {
        let opened = RegOpenKeyExW(HKEY_CURRENT_USER, key.as_ptr(), 0, KEY_SET_VALUE, &mut h);
        if opened != ERROR_SUCCESS {
            return Err(format!("打开卸载注册表键失败（{opened}）"));
        }
        let w = wide(value);
        // cbData 含结尾那个 NUL，和 NSIS 自己写这条值时的算法一致
        let set = RegSetValueExW(
            h,
            name.as_ptr(),
            0,
            REG_SZ,
            w.as_ptr() as *const u8,
            (w.len() * 2) as u32,
        );
        RegCloseKey(h);
        if set != ERROR_SUCCESS {
            return Err(format!("写 UninstallString 失败（{set}）"));
        }
        Ok(())
    }
}

#[cfg(windows)]
fn wide(s: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> Names {
        NAMES
    }

    /// 判定的三档：自家原生入口改指壳、已是壳就不再动、别家的一字不改
    #[test]
    fn classify_covers_the_three_outcomes() {
        let n = names();
        let install = Path::new("E:\\SideShift");
        // 模板写的值带引号，且 $INSTDIR 的分隔符尾注不确定 → 只认文件名
        let native = "\"E:\\SideShift\\uninstall.exe\"".to_string();
        let Entry::Rewrite(v) = classify_entry(Some(&native), &n, install, install) else {
            panic!("原生入口必须改成壳");
        };
        assert_eq!(v, "\"E:\\SideShift\\SideShift-Uninstall.exe\"");
        // 升级覆盖时读回来的已经是壳：不能再改一次，也不该报任何话
        let already = format!("\"{}\"", shell_of(install, n.shell));
        assert!(matches!(
            classify_entry(Some(&already), &n, install, install),
            Entry::Already
        ));
        // 大小写与正斜杠都不是判据（同一目录的另一种写法；own_dir 守卫只拦**别的**目录）
        assert!(matches!(
            classify_entry(Some("e:/sideshift/UNINSTALL.EXE"), &n, install, install),
            Entry::Rewrite(_)
        ));
        assert!(matches!(classify_entry(None, &n, install, install), Entry::Foreign));
        assert!(matches!(classify_entry(Some("   "), &n, install, install), Entry::Foreign));
    }

    fn shell_of(dir: &Path, name: &str) -> String {
        format!("\"{}\"", dir.join(name).display())
    }

    /// 自愈场景独有的守卫：入口指向的卸载器**不在当前 exe 旁边** ⇒ 不动手。
    /// 没有这道闸，dev 进程会把正式安装的卸载入口改指进仓库的 target 目录
    #[test]
    fn self_heal_guard_requires_the_entry_to_live_beside_the_running_exe() {
        let n = names();
        let dev_dir = Path::new("E:\\Projects\\sideshift\\target\\debug");
        let installed_dir = Path::new("C:\\Users\\me\\App\\Local\\SideShift");
        // 装好的应用的原生入口：dev 进程不许碰
        let native_elsewhere = format!("\"{}\"", installed_dir.join(n.native).display());
        assert!(matches!(
            classify_entry(Some(&native_elsewhere), &n, dev_dir, dev_dir),
            Entry::Foreign
        ));
        // 指向别处的壳同名文件：判 Already（不改写任何东西，是安全的——
        // 真正的改写只发生在原生入口上，那一条已被上面的目录守卫拦住）
        let shell_elsewhere = format!("\"{}\"", installed_dir.join(n.shell).display());
        assert!(matches!(
            classify_entry(Some(&shell_elsewhere), &n, dev_dir, dev_dir),
            Entry::Already
        ));
        // 反过来：dev 目录里自己的原生入口（测试二进制覆盖安装的场景）⇒ 改
        let native_own = format!("\"{}\"", dev_dir.join(n.native).display());
        assert!(matches!(
            classify_entry(Some(&native_own), &n, dev_dir, dev_dir),
            Entry::Rewrite(_)
        ));
    }

    /// 脏值防御：解不出文件名的入口（空壳片段、纯盘符）一律 Foreign
    #[test]
    fn malformed_entries_stay_foreign() {
        let n = names();
        let install = Path::new("E:\\SideShift");
        for bad in ["\"E:\\\"", "E:\\", "\"uninstall.exe\"", "\"\""] {
            assert!(
                matches!(classify_entry(Some(bad), &n, install, install), Entry::Foreign),
                "{bad:?} 不该被判成可改写"
            );
        }
    }
}
