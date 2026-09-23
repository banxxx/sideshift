//! 数据根目录：缓存与产物按 GB 计，默认值不该压在系统盘上。
//!
//! 决策顺序（`suggested_root`，从高到低），两套发行物共用一条链：
//! 1. **便携标记** —— exe 同级有 `portable.flag` ⇒ 数据全跟着 exe 走（`{exe}\data`）
//! 2. **安装器指定** —— 自绘安装壳装完后写进配置目录的 `installer.json`
//! 3. **非系统盘预选** —— 剩余空间最大且够用的固定盘（`{盘}\SideShift`）
//! 4. 用户目录回落（`{home}\SideShift`）
//!
//! 系统盘不进预选候选：往 `C:\SideShift` 建目录要在盘根写，普通用户没这个权限；
//! 真想放 C 盘的人走设置里的目录选择器，那是他的明确选择。

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// 数据根目录名：预选盘与用户目录回落用同一个名字，避免出现两套布局
pub const DIR_NAME: &str = "SideShift";
/// 便携发行物与安装版唯一的区分凭据（构建脚本放进 zip）
pub const PORTABLE_FLAG: &str = "portable.flag";
/// 便携模式下数据跟着 exe 走的那个目录名（配置与 output/cache 都在里面）
pub const DATA_DIR_NAME: &str = "data";
/// 自绘安装壳写入的数据根：`{"dataRoot":"E:\\Games\\SideShift"}`，只在这份文件存在时覆盖预选
pub const INSTALLER_FILE: &str = "installer.json";
/// 数据根下的两个子目录。名字放这里而不是散在 `defaults_in` 里：安装壳要在装的时候
/// 就把「以后会落在哪」显示给用户看，两边各写一遍字符串迟早分叉
pub const OUTPUT_DIR_NAME: &str = "output";
pub const CACHE_DIR_NAME: &str = "cache";

/// 低于此剩余空间的盘不参与预选：宁可用默认的用户目录，也不替用户把整盘塞满
const MIN_FREE_BYTES: u64 = 10 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Drive {
    pub letter: char,
    pub free_bytes: u64,
    pub is_system: bool,
}

impl Drive {
    /// 盘根。候选只在 Windows 上产生（其他平台 `probe` 返回空），分隔符按 Windows 口径写死
    fn root(&self) -> PathBuf {
        PathBuf::from(format!("{}:\\", self.letter))
    }

    fn data_root(&self) -> PathBuf {
        self.root().join(DIR_NAME)
    }
}

/// 数据根下的固定布局：`(产物目录, 缓存目录)`。安装壳显示的路径与应用实际用的路径共用它，
/// 否则「安装时看到的 A」和「设置页里的 B」会各算各的
pub fn layout_in(root: &Path) -> (PathBuf, PathBuf) {
    (
        root.join(OUTPUT_DIR_NAME),
        root.join(CACHE_DIR_NAME),
    )
}

/// 本机可用盘，按剩余空间从多到少。非 Windows 返回空 → 一律回落用户目录
pub fn probe() -> Vec<Drive> {
    let mut drives = probe_native();
    drives.sort_by_key(|d| std::cmp::Reverse(d.free_bytes));
    drives
}

/// 有资格当数据根的盘：非系统盘且剩余空间够用。
/// 系统盘不给：往 `C:\` 盘根建目录普通用户没权限，给了就是给一个必失败的选项
fn eligible(d: &Drive) -> bool {
    !d.is_system && d.free_bytes >= MIN_FREE_BYTES
}

/// 可参与预选的盘：非系统盘且剩余空间够用里挑最大的。
/// 自己取 max，不靠 `probe` 的排序——解耦后调用方乱序传进来也不会选错
fn preselectable(drives: &[Drive]) -> Option<&Drive> {
    drives.iter().filter(|d| eligible(d)).max_by_key(|d| d.free_bytes)
}

/// 一个候选盘档位：标签、可用字节、以及该盘上的固定布局。
/// 给安装壳列表用；「默认放哪」与「列表里显示什么」必须是同一份数据，否则
/// 安装时选的目录和应用实际用的目录会分叉
///
/// 唯一的消费者是安装壳（`installer/src/main.rs` 用 `#[path]` 编走本文件），
/// 主应用这边的 dead-code 检测看不到它 ⇒ allow 不是"暂时没用到"，是"另一棵编译树在用"
#[allow(dead_code)]
pub struct Offer {
    pub label: String,
    pub free_bytes: u64,
    pub data_root: PathBuf,
    pub output_dir: PathBuf,
    pub cache_dir: PathBuf,
}

/// 安装壳的候选盘：够用的非系统固定盘，按剩余空间降序 ⇒ 第一个就是预选档
#[allow(dead_code)]
pub fn offers() -> Vec<Offer> {
    probe()
        .iter()
        .filter(|d| eligible(d))
        .map(|d| {
            let root = d.data_root();
            let (output_dir, cache_dir) = layout_in(&root);
            Offer {
                label: format!("{}:", d.letter),
                free_bytes: d.free_bytes,
                data_root: root,
                output_dir,
                cache_dir,
            }
        })
        .collect()
}

/// 给定 exe 所在目录，判断是不是便携包（纯函数，测试好打）
fn portable_root_in(exe_dir: &Path) -> Option<PathBuf> {
    exe_dir
        .join(PORTABLE_FLAG)
        .is_file()
        .then(|| exe_dir.join(DATA_DIR_NAME))
}

/// 便携模式下的数据根（exe 同级的 `data`）；非便携返回 None。
/// 判据是标记文件而不是"猜自己有没有被装进 Program Files"：安装版被人拷到 U 盘时，
/// 猜法让两套语义都不成立，而标记由发行物自己声明，谁的包走谁的路。
/// 结果缓存在 OnceLock 里 —— exe 位置不会中途变，且配置目录要处处用它（见 persist::config_dir）
pub fn portable_root() -> Option<PathBuf> {
    static CACHED: OnceLock<Option<PathBuf>> = OnceLock::new();
    CACHED
        .get_or_init(|| {
            let exe_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
            portable_root_in(&exe_dir)
        })
        .clone()
}

/// 自绘安装壳交给我们的数据根（`{配置目录}\installer.json`）。
/// 读不到 / 字段为空 / JSON 坏 / 盘不在了 ⇒ 当作没有，回落预选：
/// 一个可选的偏好文件不能有能力把默认值变成空串，也不能把人留在拔掉的盘上
fn installer_root(config_dir: &Path) -> Option<PathBuf> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Installer {
        #[serde(default)]
        data_root: String,
    }
    let text = std::fs::read_to_string(config_dir.join(INSTALLER_FILE)).ok()?;
    let parsed: Installer = serde_json::from_str(&text).ok()?;
    let root = PathBuf::from(parsed.data_root.trim());
    // 检查的是**盘根**而不是这个目录本身：安装那一刻它还不存在，直接 exists() 会把刚选好的偏好丢掉。
    // `ancestors().last()` 对 `E:\Games\SideShift` 得到 `E:\`，对相对路径得到空串（不存在）⇒ 一并拒掉
    let on_live_drive = root.ancestors().last().is_some_and(|r| r.exists());
    on_live_drive.then_some(root)
}

/// 数据根：便携 → 安装器指定 → 非系统盘预选 → 用户目录
pub fn suggested_root(home: &Path, config_dir: &Path) -> PathBuf {
    if let Some(root) = portable_root() {
        return root;
    }
    if let Some(root) = installer_root(config_dir) {
        return root;
    }
    match preselectable(&probe()) {
        Some(d) => d.data_root(),
        None => home.join(DIR_NAME),
    }
}

/// windows-sys 不把 `DRIVE_*` 随 `GetDriveTypeW` 一起导出，这里按 Win32 文档取 FIXED=3。
/// 只认固定盘：可移动盘/光驱的容量不可靠，网络盘探测时可能整段卡住
#[cfg(windows)]
const DRIVE_FIXED: u32 = 3;

#[cfg(windows)]
fn free_space(root: &[u16]) -> Option<u64> {
    use windows_sys::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetDriveTypeW};

    if unsafe { GetDriveTypeW(root.as_ptr()) } != DRIVE_FIXED {
        return None;
    }
    let mut available = 0u64;
    let mut total = 0u64;
    let mut free = 0u64;
    let ok = unsafe { GetDiskFreeSpaceExW(root.as_ptr(), &mut available, &mut total, &mut free) };
    // 返回 0 是失败；available 才是「当前用户可用」（配额盘上 total 会骗人）
    if ok == 0 {
        return None;
    }
    Some(available)
}

#[cfg(windows)]
fn probe_native() -> Vec<Drive> {
    use std::os::windows::ffi::OsStrExt;

    let system = std::env::var("SystemDrive")
        .ok()
        .and_then(|v| v.trim_end_matches(':').chars().next())
        .unwrap_or('C')
        .to_ascii_uppercase();

    (b'A'..=b'Z')
        .map(|c| (c as char).to_ascii_uppercase())
        .filter_map(|letter| {
            let root: Vec<u16> = std::ffi::OsStr::new(&format!("{}:\\", letter))
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            free_space(&root).map(|free| Drive {
                letter,
                free_bytes: free,
                is_system: letter == system,
            })
        })
        .collect()
}

#[cfg(not(windows))]
fn probe_native() -> Vec<Drive> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(letter: char, free_gb: u64, is_system: bool) -> Drive {
        Drive {
            letter,
            free_bytes: free_gb * 1024 * 1024 * 1024,
            is_system,
        }
    }

    /// 系统盘再大也不能预选——这正是本次改动要修的默认值
    #[test]
    fn never_preselects_system_drive() {
        let drives = [d('C', 900, true), d('E', 300, false), d('D', 500, false)];
        assert_eq!(preselectable(&drives).map(|x| x.letter), Some('D'));
    }

    /// 装不下一个整合包的盘不参与预选，宁回落用户目录
    #[test]
    fn skips_drives_below_floor() {
        let drives = [
            d('C', 900, true),
            d('D', MIN_FREE_BYTES / 1024 / 1024 / 1024 - 1, false),
        ];
        assert_eq!(preselectable(&drives), None);
    }

    #[test]
    fn no_drive_preselects_nothing() {
        assert_eq!(preselectable(&[]), None);
    }

    /// 真实探测的 FFI 探针（只在 Windows 跑）：系统盘必然以固定盘身份出现在候选里。
    /// 宽字符串没终止符、PCWSTR 传成裸指针这类错不会编译失败，而是 26 个盘全部探测不通
    /// → 预选静默失效，界面只表现为「没有别的盘」。这种账必须由测试拦住，不能靠肉眼看路径。
    #[test]
    #[cfg(windows)]
    fn probe_actually_reads_the_real_drives() {
        let system = std::env::var("SystemDrive")
            .unwrap_or_else(|_| "C:".into())
            .trim_end_matches(':')
            .to_ascii_uppercase();
        let drives = probe();
        let letters: Vec<String> = drives.iter().map(|d| d.letter.to_string()).collect();
        assert!(
            letters.contains(&system),
            "没探到系统盘 {system}（候选 {letters:?}）→ GetDriveTypeW/GetDiskFreeSpaceExW 探针失效"
        );
        assert!(
            drives.iter().any(|d| d.free_bytes > 0),
            "所有盘剩余空间都是 0 → 容量读不到（候选 {letters:?}）"
        );
    }

    /// 数据根串必须全本机分隔符：混着 `/` 的路径交给 opener 会打不开
    #[test]
    #[cfg(windows)]
    fn drive_root_uses_backslash() {
        let drive = d('D', 100, false);
        assert_eq!(drive.root(), PathBuf::from("D:\\"));
        assert_eq!(drive.data_root(), PathBuf::from("D:\\SideShift"));
    }

    #[test]
    fn suggested_root_falls_back_to_home_without_drives() {
        let home = Path::new(if cfg!(windows) { "C:\\Users\\ban" } else { "/home/ban" });
        let root = suggested_root(home, &home.join("config"));
        // 有非系统盘时不碰 home；没有则回落到 home\SideShift
        if preselectable(&probe()).is_none() {
            assert_eq!(root, home.join(DIR_NAME));
        }
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sideshift-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 安装壳写的数据根：目录在那一刻还不存在，所以只能查**盘根**在不在，
    /// 不能查目录本身（用 exists() 会把刚选好的偏好丢掉，等于选盘白选）
    #[test]
    fn installer_root_accepts_only_a_usable_absolute_path() {
        let config = temp_dir("installer-root");
        let wanted = config.join("chosen").join("SideShift");
        let read = |json: &str| {
            std::fs::write(config.join(INSTALLER_FILE), json).unwrap();
            installer_root(&config)
        };
        assert_eq!(
            read(&format!("{{\"dataRoot\": {:?}}}", wanted.display().to_string())),
            Some(wanted.clone()),
            "还不存在的数据根必须照收"
        );
        assert_eq!(read(r#"{"dataRoot": ""}"#), None, "空串不能顶掉默认值");
        assert_eq!(read("not json"), None, "坏 JSON 不能把默认值搞坏");
        assert_eq!(read(r#"{"dataRoot": "relative/place"}"#), None, "相对路径不可信");
        assert_eq!(read(r#"{"dataRoot": "Z:\\nope"}"#), None, "不在了的盘不能把人留在原地");
        assert_eq!(read("{}"), None, "缺字段按没配过处理");
        std::fs::remove_dir_all(config).ok();
    }

    /// 便携标记决定一切：有 flag 才进便携，data 目录由应用自己建（zip 里不预置空目录）
    #[test]
    fn portable_flag_decides_the_data_root() {
        let dir = temp_dir("portable-flag");
        assert_eq!(portable_root_in(&dir), None, "没有标记文件就不是便携包");
        std::fs::write(dir.join(PORTABLE_FLAG), "").unwrap();
        assert_eq!(
            portable_root_in(&dir),
            Some(dir.join(DATA_DIR_NAME)),
            "便携包的数据根必须落在 exe 同级"
        );
        std::fs::remove_dir_all(dir).ok();
    }
}
