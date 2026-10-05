//! 数据根目录：缓存与产物按 GB 计，默认值不该压在系统盘上。
//! `suggested_root` 决策顺序（高→低）：exe 同级 `portable.flag` ⇒ `{exe}\data` > 安装器写的 `installer.json` >
//! 剩余空间最大且够用的非系统盘 `{盘}\SideShift` > 回落 `{home}\SideShift`。系统盘不进预选（盘根建目录没权限），
//! 想放 C 盘走设置里的目录选择器。

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// 数据根目录名：预选盘与用户目录回落用同一个名字，避免出现两套布局
pub const DIR_NAME: &str = "SideShift";
/// 便携发行物与安装版唯一的区分凭据（构建脚本放进 zip）
pub const PORTABLE_FLAG: &str = "portable.flag";
/// 便携模式下数据跟着 exe 走的那个目录名（配置与 output/cache 都在里面）
pub const DATA_DIR_NAME: &str = "data";
/// 安装版里「应用自己的状态」放的目录名（settings/tasks/鸣谢快照/皮肤副本/WebView profile）。
/// 和便携包的 `data` 刻意分开：那个是「配置 + 产物 + 缓存」整包跟着 exe 走；这个只有配置，
/// 产物与缓存仍按数据根那套走（`suggested_root`）。安装壳要在同一个位置写 installer.json，
/// 所以名字放这里而不是两边各写一遍
pub const APP_DATA_DIR_NAME: &str = "appdata";
/// 自绘安装壳写入的数据根：`{"dataRoot":"E:\\Games\\SideShift"}`，只在这份文件存在时覆盖预选
pub const INSTALLER_FILE: &str = "installer.json";
/// 全局设置那份。卸载壳要读它里面的 `cacheDir`：人在设置里改过目录的话，默认布局算出来的
/// 那个 `cache` 就不是缓存，照着删等于删错目录。名字在这里单源，而不是主应用与壳各写一遍
pub const SETTINGS_FILE: &str = "settings.json";
/// 数据根下的两个子目录。名字放这里而不是散在 `defaults_in` 里：安装壳要在装的时候
/// 就把「以后会落在哪」显示给用户看，两边各写一遍字符串迟早分叉
pub const OUTPUT_DIR_NAME: &str = "output";
pub const CACHE_DIR_NAME: &str = "cache";
/// 卸载壳装进安装目录时用的文件名。写它的是安装壳、登记它的是 NSIS 的 `UninstallString`、
/// 删它的是卸载钩子——三方认的是同一个名字，所以单源在这里（钩子那侧只能写字面量，改这里要同步改 `nsis/hooks.nsh`）。
/// 主应用这边 dead_code：唯一的消费者是安装壳，它用 `#[path]` 编走本文件
#[allow(dead_code)]
pub const UNINSTALL_SHELL_NAME: &str = "SideShift-Uninstall.exe";
/// NSIS 模板自己那份卸载器的**原名**（模板 :650 `WriteUninstaller "$INSTDIR\uninstall.exe"`）。
/// 安装完它会被收进配置目录（见 `NSIS_UNINSTALLER_STASHED`），但卸载壳两处都认，
/// 所以这两个名字都得留着
#[allow(dead_code)]
pub const NSIS_UNINSTALLER_NAME: &str = "uninstall.exe";
/// 原生卸载器收起来之后叫的名字，躺在 `appdata\` 里。**为什么不干脆删掉它**：快捷方式
/// （含从任务栏/开始菜单取消固定）、注册表项、文件清单都是它的删除清单在管，卸载壳只是把
/// 界面换成我们这套、真正动手的仍是它；删了它等于把这些清单抄一份到 Rust，抄漏一项就是
/// 用户机器上删不掉的残留。留在安装目录里则像第二个卸载入口，所以收进我们自己的目录
#[allow(dead_code)]
pub const NSIS_UNINSTALLER_STASHED: &str = "nsis-uninstall.exe";

/* ---------------- 缓存目录里「我们名下」的条目 ---------------- */

/// 四个缓存桶：`{cache}\{名}\`，每个都装一类可再生数据，设置页那张卡按桶清理。
/// 名字单源在这里而不是各写入方各留一份：卸载壳能编到的只有本文件（`#[path]` 共享面），
/// 它要删的清单必须和写入方建目录时用的是同一份字符串。写入方从本文件 `pub use` 回去
pub const CACHE_FILES_DIR: &str = "files";
pub const CACHE_TASKS_DIR: &str = "tasks";
pub const CACHE_INSTALLS_DIR: &str = "installs";
pub const CACHE_UPDATE_DIR: &str = "update";
/// 三张躺在缓存根（不在任何桶里）的索引：`cf-files-index.json` / `env-index.json` /
/// `java-index.json`。它们刻意放在可清理目录**之外**（不可再生或再生很贵），但字节确实
/// 是我们在缓存根里落的 ⇒ 卸载要收，设置页的清理不认它们
pub const CACHE_CF_INDEX: &str = "cf-files-index.json";
pub const CACHE_ENV_INDEX: &str = "env-index.json";
pub const CACHE_JAVA_INDEX: &str = "java-index.json";

/// 卸载壳在缓存目录里**只允许删**这些名字，表外的字节一个不碰。判据来自人在设置里选的
/// 目录可能正是他自己的东西（`D:\Games`、`Documents`），照着整目录递归删等于把用户文件带走
///
/// 新增缓存类目必须同时登记进这里，否则卸载完留在用户机器上没人收
#[allow(dead_code)]
pub const CACHE_OWNED: &[&str] = &[
    CACHE_FILES_DIR,
    CACHE_TASKS_DIR,
    CACHE_INSTALLS_DIR,
    CACHE_UPDATE_DIR,
    CACHE_CF_INDEX,
    CACHE_ENV_INDEX,
    CACHE_JAVA_INDEX,
];

/// 归属标记：躺在这颗目录里的缓存目录才「是我们建的」。由 [claim_cache_root] 盖，
/// 卸载壳读它决定敢不敢动手（见那里对判据的解释）
#[allow(dead_code)]
pub const CACHE_MARKER: &str = ".sideshift-cache";

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

/// 给缓存目录盖上「这个目录是我们建的」标记，并确保目录本身在（安装壳靠这一句建出缓存目录）。
/// 已经有标记就什么都不做。
///
/// 判据为什么必须是非递归的 `create_dir`：人在设置里可以把缓存指到任何**已存在**的目录
/// （`D:\Games`、`C:\Users\me\Documents`），那些目录不是我们造的，一旦盖章就等于授权卸载壳
/// 往里面删东西。`create_dir_all` 分不出最后一段是它建的还是本来就在 ⇒ 不能用它判定。
/// 父段不存在时 `create_dir` 失败，这时整条链都只能是本次造的 ⇒ 再走 `create_dir_all`，
/// 成功同样有资格盖章。
///
/// 标记只覆盖「装上/改目录那一刻」；在这之前缓存目录就已经存在的老安装拿不到它，卸载壳对那种
/// 情况按「缓存目录正好等于规则算出来的那个」放行，见 `uninstaller` 的归属判定。
///
/// 主应用与安装壳盖标记、卸载壳只读那颗文件 ⇒ 本文件被卸载壳 `#[path]` 编走时这里是死代码
#[allow(dead_code)]
pub fn claim_cache_root(cache_dir: &Path) -> std::io::Result<()> {
    let marker = cache_dir.join(CACHE_MARKER);
    if marker.is_file() {
        return Ok(());
    }
    let created = match std::fs::create_dir(cache_dir) {
        Ok(()) => true,
        // 本来就在：不是本次造的，不盖章，但这不算失败
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => false,
        Err(_) => !cache_dir.exists() && std::fs::create_dir_all(cache_dir).is_ok(),
    };
    // 盖不上标记只是让卸载时少删一点，不值得让装好的包报错
    if created {
        let _ = std::fs::write(&marker, b"");
    }
    Ok(())
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
/// 一个可选的偏好文件不能有能力把默认值变成空串，也不能把人留在拔掉的盘上。
/// 卸载壳也调它回显「产物留在哪」——两边读的是同一个偏好，不会一个显示 E:\ 一个用 F:\
pub fn installer_root(config_dir: &Path) -> Option<PathBuf> {
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

    /// 归属标记只在「这个缓存目录是本次建出来的」时盖。人已存在的目录（设置里指到
    /// `D:\Games` 那一类）绝不能盖章——盖了等于授权卸载壳往用户自己的地盘里删东西
    #[test]
    fn claim_stamps_only_a_directory_it_created() {
        let base = temp_dir("claim");
        let fresh = base.join(CACHE_DIR_NAME);
        claim_cache_root(&fresh).unwrap();
        assert!(fresh.is_dir(), "缓存目录该由这一次调用建出来");
        assert!(
            fresh.join(CACHE_MARKER).is_file(),
            "本次建的目录必须盖上归属标记"
        );

        // 已经存在的目录：不盖章，也不报错（人在设置里指到自己已有的文件夹就是这个形态）
        let pre = base.join("already");
        std::fs::create_dir_all(&pre).unwrap();
        claim_cache_root(&pre).unwrap();
        assert!(
            !pre.join(CACHE_MARKER).exists(),
            "本来就在的目录不是我们造的，不能盖章"
        );

        // 父段也不存在 ⇒ 整条链都是本次造的，照样有资格盖章（安装壳第一次装就是这一档）
        let deep = base.join("E").join(DIR_NAME).join(CACHE_DIR_NAME);
        claim_cache_root(&deep).unwrap();
        assert!(deep.is_dir());
        assert!(deep.join(CACHE_MARKER).is_file(), "整条链都是本次建的就该盖章");

        // 已有标记 ⇒ 幂等，不重复建、不重复写
        claim_cache_root(&fresh).unwrap();
        assert!(fresh.join(CACHE_MARKER).is_file());
        std::fs::remove_dir_all(base).ok();
    }

    /// 卸载壳的删除清单必须列全四个桶 + 三张根索引：漏一条就是卸载后留在用户机器上没人收
    #[test]
    fn owned_cache_entries_list_buckets_and_indexes() {
        assert_eq!(
            CACHE_OWNED,
            &[
                CACHE_FILES_DIR,
                CACHE_TASKS_DIR,
                CACHE_INSTALLS_DIR,
                CACHE_UPDATE_DIR,
                CACHE_CF_INDEX,
                CACHE_ENV_INDEX,
                CACHE_JAVA_INDEX,
            ]
        );
        assert_eq!(
            CACHE_OWNED.len(),
            CACHE_OWNED
                .iter()
                .copied()
                .collect::<std::collections::HashSet<&str>>()
                .len(),
            "清单里有重名"
        );
    }
}
