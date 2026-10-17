//! 验证副本：把任务产物的 zip 解到 `{cache}\verify\{task_id}\`，本机开服验证的读写都落在这份副本里。
//! - 与 `core::verify`（打包后离线对账、零子进程、不落地）是两回事：这一档要长驻、要被人改。
//! - 幂等哨兵 `.verify-copy.json` 记的是**交付包绝对路径 + 字节 + mtime**：三项都对上才复用。
//!   包被重新导出过（同路径原地覆盖）就整份重解——留着旧字节等于在验一份不存在的包，
//!   代价是上次在副本里做过的启停跟着清零，那正是「换包」该有的人事。
//! - 停用 = 同目录改名 `x.jar` ⇄ `x.jar.disabled`：改完它不再是 `.jar`，三家加载器都不认它。
//!   只动 `mods/` 顶层那一层，名字里带分隔符一律拒。
//! - 副本里唯一会被改写的配置文件是 `server.properties` 的 `server-port`（本机端口被别的程序
//!   占用时换一个空闲的，交付包那份不动）；其余配置一律只读。

use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::core::cleanup::dir_bytes;
use crate::core::data_root::CACHE_VERIFY_DIR;
use crate::models::VerifyModEntry;

/// 副本根里那颗幂等哨兵。以 `.` 开头 ⇒ 服务端与加载器都不会读它，也不会被算进模组清单
const MANIFEST: &str = ".verify-copy.json";
/// 停用后缀：加在整串文件名后面（`x.jar` → `x.jar.disabled`），还原时只削掉这一段
pub const DISABLED_SUFFIX: &str = ".disabled";
/// 副本里的模组目录名：产物的顶层就是包根，所以它是解压根的直接子目录
pub const MODS_DIR: &str = "mods";

/// 哨兵内容：交付包身份 + 本次解出来的规模
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct Stamp {
    /// 交付包的**绝对路径**：按文件名索引会把两个同名不同目录的包当成同一个（与包身份同一条口径）
    source: String,
    source_bytes: u64,
    source_modified_ms: i64,
    files: usize,
    created_at: i64,
}

/// 解包结果
#[derive(Debug, Clone)]
pub struct Prepared {
    pub dir: PathBuf,
    pub files: usize,
    pub bytes: u64,
    /// 命中哨兵、这次一个字节都没重解
    pub reused: bool,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

/// 副本目录。**task_id 从前端来**，而这里的每个操作都会拼路径（还包含递归删除），
/// 所以带分隔符、`..`、盘符或空串的一律不拼：只认我们自己生成过的那形状
pub fn copy_dir(cache_dir: &Path, task_id: &str) -> Result<PathBuf, String> {
    check_id(task_id)?;
    Ok(cache_dir.join(CACHE_VERIFY_DIR).join(task_id))
}

fn check_id(task_id: &str) -> Result<(), String> {
    let clean = !task_id.is_empty()
        && task_id.len() <= 64
        && task_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if clean {
        Ok(())
    } else {
        Err(format!("任务 id「{task_id}」不是可用的目录名，验证没有动任何文件"))
    }
}

/// 交付包的身份：绝对路径 + 字节 + mtime（毫秒）。三项都在 `metadata` 里，取一次
fn stamp_of(zip_path: &Path, source_bytes: u64, source_modified_ms: i64) -> Stamp {
    Stamp {
        source: zip_path.to_string_lossy().to_string(),
        source_bytes,
        source_modified_ms,
        files: 0,
        created_at: now_ms(),
    }
}

fn modified_ms(t: SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

fn read_stamp(dir: &Path) -> Option<Stamp> {
    let text = fs::read_to_string(dir.join(MANIFEST)).ok()?;
    serde_json::from_str(&text).ok()
}

fn write_stamp(dir: &Path, stamp: &Stamp) -> Result<(), String> {
    let text = serde_json::to_string_pretty(stamp)
        .map_err(|e| format!("验证副本的哨兵写不出来：{e}"))?;
    fs::write(dir.join(MANIFEST), text).map_err(|e| format!("验证副本的哨兵写不进去：{e}"))
}

/// 把交付包解成副本。命中哨兵直接复用；对不上就整份删掉重解。
///
/// 删整份而不是增量对齐：产物 zip 是 builder 全量写的，条目只会多不会少，
/// 而「上次有、这次没有」的那一枚留在副本里就是服务端真会加载的一枚模组。
pub fn prepare(cache_dir: &Path, task_id: &str, zip_path: &Path) -> Result<Prepared, String> {
    let dest = copy_dir(cache_dir, task_id)?;
    let meta = fs::metadata(zip_path)
        .map_err(|_| format!("交付包不在了：{}", zip_path.display().to_string()))?;
    if !meta.is_file() {
        return Err(format!("交付包路径指向的不是文件：{}", zip_path.display()));
    }
    let mut stamp = stamp_of(zip_path, meta.len(), meta.modified().map(modified_ms).unwrap_or_default());

    if let Some(old) = read_stamp(&dest) {
        let same = old.source == stamp.source
            && old.source_bytes == stamp.source_bytes
            && old.source_modified_ms == stamp.source_modified_ms;
        if same && dest.is_dir() {
            let bytes = dir_bytes(&dest);
            return Ok(Prepared {
                dir: dest,
                files: old.files,
                bytes,
                reused: true,
            });
        }
    }

    if dest.exists() {
        fs::remove_dir_all(&dest)
            .map_err(|e| format!("旧的验证副本删不掉（里面有文件正被占用？）：{e}"))?;
    }
    fs::create_dir_all(&dest).map_err(|e| format!("验证副本目录建不出来：{e}"))?;

    let (files, bytes) = extract(zip_path, &dest)?;
    stamp.files = files;
    write_stamp(&dest, &stamp)?;
    Ok(Prepared { dir: dest, files, bytes, reused: false })
}

/// 逐条目解包。三条拒：符号链接、`enclosed_name()` 认不下的路径、以及**空名字**条目
/// （根条目 `""` 解出来就是副本目录本身，写它等于把哨兵盖住）
fn extract(zip_path: &Path, dest: &Path) -> Result<(usize, u64), String> {
    let file = File::open(zip_path).map_err(|e| format!("交付包打不开：{e}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("交付包不是可用的 zip：{e}"))?;
    let mut files = 0usize;
    let mut bytes = 0u64;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("交付包条目读不动：{e}"))?;
        if entry.is_symlink() {
            return Err(format!(
                "交付包里含有符号链接条目 {}，验证没有解包",
                entry.name()
            ));
        }
        if entry.is_dir() {
            continue;
        }
        let Some(rel) = entry.enclosed_name() else {
            return Err(format!(
                "交付包里有条目跑到包根外面（{}），验证没有解包",
                entry.name()
            ));
        };
        if rel.as_os_str().is_empty() {
            continue;
        }
        let target = dest.join(&rel);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("验证副本建目录失败（{}）：{e}", parent.display()))?;
        }
        let mut out = BufWriter::new(
            File::create(&target).map_err(|e| format!("验证副本写文件失败（{rel:?}）：{e}"))?,
        );
        let written = std::io::copy(&mut entry, &mut out)
            .map_err(|e| format!("验证副本解到一半失败（{rel:?}）：{e}"))?;
        out.flush().map_err(|e| format!("验证副本落盘失败（{rel:?}）：{e}"))?;
        files += 1;
        bytes += written;
    }
    Ok((files, bytes))
}

/// 回收副本（含哨兵）。目录本来就不在算成功：清理要能反复点
pub fn teardown(cache_dir: &Path, task_id: &str) -> Result<(), String> {
    let dir = copy_dir(cache_dir, task_id)?;
    match fs::remove_dir_all(&dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("验证副本删不掉（进程还在写它？）：{e}")),
    }
}

/// 启动清扫：注册表里已无此任务的副本目录收走。与 `sweep_task_staging` 同一判据、
/// 同一个调用点，否则「删掉任务」只在 `clear_trash` 那一路清暂存，副本会永远留在盘上
pub fn sweep_orphans(cache_dir: &Path, keep: impl Fn(&str) -> bool) {
    let root = cache_dir.join(CACHE_VERIFY_DIR);
    let Ok(entries) = fs::read_dir(&root) else {
        return;
    };
    for entry in entries.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if !keep(&name) {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
    // 整个 verify 根空了就一起收，免得设置页永远露一个空壳
    if dir_is_empty(&root) {
        let _ = fs::remove_dir(&root);
    }
}

fn dir_is_empty(dir: &Path) -> bool {
    match fs::read_dir(dir) {
        Ok(mut rd) => rd.next().is_none(),
        Err(_) => false,
    }
}

/// 副本里的模组清单：只认 `mods/` 顶层的 `.jar`（启用）与 `.jar.disabled`（停用），
/// 其余字节既不算模组也不许启停。按去掉 `.disabled` 后的名字排，切档时两档顺序一致
pub fn list_mods(cache_dir: &Path, task_id: &str) -> Result<Vec<VerifyModEntry>, String> {
    let dir = copy_dir(cache_dir, task_id)?;
    Ok(read_mods(&dir))
}

fn read_mods(copy_dir: &Path) -> Vec<VerifyModEntry> {
    let mods = copy_dir.join(MODS_DIR);
    let Ok(entries) = fs::read_dir(&mods) else {
        return Vec::new();
    };
    let mut out: Vec<VerifyModEntry> = Vec::new();
    for entry in entries.flatten() {
        if let Some(m) = mod_entry(&entry.path(), &entry.file_name().to_string_lossy()) {
            out.push(m);
        }
    }
    out.sort_by(|a, b| {
        a.name
            .to_ascii_lowercase()
            .cmp(&b.name.to_ascii_lowercase())
            .then_with(|| a.file.cmp(&b.file))
    });
    out
}

/// 一个磁盘名字 → 模组条目；不是那两种后缀就给 None（这个函数是「什么算模组」唯一的判据处）
fn mod_entry(path: &Path, file: &str) -> Option<VerifyModEntry> {
    if !path.is_file() {
        return None;
    }
    let (name, enabled) = match file.strip_suffix(DISABLED_SUFFIX) {
        Some(stripped) if is_jar(stripped) => (stripped.to_string(), false),
        _ if is_jar(file) => (file.to_string(), true),
        _ => return None,
    };
    Some(VerifyModEntry {
        file: file.to_string(),
        name,
        enabled,
        size_bytes: path.metadata().map(|m| m.len()).unwrap_or_default(),
    })
}

fn is_jar(name: &str) -> bool {
    name.len() > 4 && name.to_ascii_lowercase().ends_with(".jar")
}

/// 启停一枚模组：`file` 必须是清单里那个**磁盘名字**（停用档传过来的是带 `.disabled` 的那个）。
/// 已经是想要的那一态就原样返回条目，不报错——界面上的开关与盘上的事实对不上时才需要吭声
pub fn set_mod_enabled(
    cache_dir: &Path,
    task_id: &str,
    file: &str,
    enabled: bool,
) -> Result<VerifyModEntry, String> {
    let dir = copy_dir(cache_dir, task_id)?;
    if file.is_empty()
        || file.contains('/')
        || file.contains('\\')
        || file.contains("..")
        || file.contains('\0')
        || file == DISABLED_SUFFIX
    {
        return Err(format!("模组文件名「{file}」不是 mods 目录里的名字，没有改动"));
    }
    let current = mod_entry(&dir.join(MODS_DIR).join(file), file)
        .ok_or_else(|| format!("「{file}」不是 mods 目录里的模组文件，没有改动"))?;
    if current.enabled == enabled {
        return Ok(current);
    }
    let target_name = if enabled {
        current
            .file
            .strip_suffix(DISABLED_SUFFIX)
            .map(str::to_string)
            .unwrap_or_else(|| current.file.clone())
    } else {
        format!("{}{}", current.file, DISABLED_SUFFIX)
    };
    let mods = dir.join(MODS_DIR);
    let to = mods.join(&target_name);
    if to.exists() {
        // 副本里同名两份并存是可能的（包里本来就带了一枚 .jar.disabled）：
        // 改名会静默吃掉另一枚，那是丢文件，比启停失败严重
        return Err(format!(
            "mods 目录里已经有一份 {target_name}，改名会盖掉它；请先在清单里把它处理掉"
        ));
    }
    fs::rename(mods.join(&current.file), &to)
        .map_err(|e| format!("{target_name} 改不动：{e}"))?;
    mod_entry(&to, &target_name).ok_or_else(|| format!("{target_name} 改完不认识它是不是模组"))
}

/// 副本里 `mods/` 的模组数与停用数（会话快照与启停结果共用，别两处各数一遍）
pub fn mod_counts(copy_dir: &Path) -> (u32, u32) {
    let mods = read_mods(copy_dir);
    let disabled = mods.iter().filter(|m| !m.enabled).count();
    (mods.len() as u32, disabled as u32)
}

/// 副本 `server.properties` 里的 `server-port`。读不到（没有这行、值不是数字、没这个文件）
/// 给 None —— 页面那一格写「未取到」而不是替服务端补一个 25565
pub fn server_port(copy_dir: &Path) -> Option<u16> {
    let text = fs::read_to_string(copy_dir.join("server.properties")).ok()?;
    for line in text.lines() {
        let l = line.trim_end_matches(['\r']);
        // `#` 注释行与没有等号的行都跳过：整份文件读不出端口才给 None
        let Some((key, value)) = l.split_once('=') else {
            continue;
        };
        if key.trim() == "server-port" {
            return value.trim().parse::<u16>().ok();
        }
    }
    None
}

/// 这个端口本机还空着吗：绑一次就知道——服务端起来要占的就是同一个端口，
/// 别的进程先 bind 上了，它就是会以「绑定失败」退出，那口锅本来不该扣在包上
pub fn port_in_use(port: u16) -> bool {
    std::net::TcpListener::bind((std::net::Ipv4Addr::UNSPECIFIED, port)).is_err()
}

/// 一个当前空闲的端口：交给系统分配（bind 端口 0 再放开），不猜号段、不设扫描上限。
/// 放开头尾有一小段窗口可能被别人抢去，所以这只用于改写副本配置，不承诺一定绑得上
pub fn free_port() -> Option<u16> {
    std::net::TcpListener::bind((std::net::Ipv4Addr::UNSPECIFIED, 0))
        .ok()
        .and_then(|l| l.local_addr().ok())
        .map(|a| a.port())
}

/// 改副本那份 `server.properties` 的 `server-port`，其余行原样留着。
/// 文件不在（包里没带，服务端首启会自己生成默认那份）就新建只带这一条的文件。
pub fn set_server_port(copy_dir: &Path, port: u16) -> Result<(), String> {
    const KEY: &str = "server-port";
    let path = copy_dir.join("server.properties");
    let text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("server.properties 读不动：{e}")),
    };
    let line = format!("{KEY}={port}");
    if text.is_empty() {
        return fs::write(&path, format!("{line}\n"))
            .map_err(|e| format!("server.properties 建不出来：{e}"));
    }
    let mut out = String::new();
    let mut hit = false;
    for l in text.lines() {
        // 与 `server_port` 同一个认法：按第一个等号拆、键 trim，注释行自然不命中
        let is_key = l.split_once('=').is_some_and(|(k, _)| k.trim() == KEY);
        hit |= is_key;
        out.push_str(if is_key { &line } else { l.trim_end_matches('\r') });
        out.push('\n');
    }
    if !hit {
        out.push_str(&line);
        out.push('\n');
    }
    fs::write(&path, out).map_err(|e| format!("server.properties 改不动：{e}"))
}

/// 交付包里那份 `eula.txt` 是否已同意协议：`None` = 包根没有这个文件（ builder 恒生成，
/// 没有就是这份包被人动过）。验证**不会**替包写 `eula=true`——那是替用户同意 Mojang 的协议。
/// 读的是 zip 里的原文，所以这一判在解包之前就能给出。
pub fn eula_agreed(zip_path: &Path) -> Option<bool> {
    let file = File::open(zip_path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;
    let mut text = String::new();
    archive
        .by_name("eula.txt")
        .ok()?
        .read_to_string(&mut text)
        .ok()?;
    Some(text.lines().any(|l| {
        // 按 `key=value` 拆再 trim：手写的那份等号两边常带空格，整行比对会把它当成没同意
        l.split_once('=')
            .is_some_and(|(k, v)| k.trim().eq_ignore_ascii_case("eula") && v.trim() == "true")
    }))
}

/// 「验证参数」卡要的两条包内线索：内存上限那条 JVM 参数、`online-mode`。
/// 读不到一律 `None`——界面那格写「未取到」，不替脚本补一个默认值
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PackHints {
    /// 启动命令里的内存上限，整串参数原样给（形如 `-Xmx4096M`）
    pub xmx: Option<String>,
    pub online_mode: Option<bool>,
}

/// 内存上限可能落在的几处：新式加载器走 `user_jvm_args.txt`，Fabric 与老 Forge 直接写在
/// 脚本命令行上，首启自装那一档真正常跑的那句在安装器生成的 `run.*` 里
const HINT_SCRIPTS: [&str; 5] = [
    "user_jvm_args.txt",
    "start.bat",
    "run.bat",
    "start.sh",
    "run.sh",
];

fn xmx_token<'a>(mut words: impl Iterator<Item = &'a str>) -> Option<String> {
    words
        .find(|w| w.strip_prefix("-Xmx").is_some_and(|v| !v.is_empty()))
        .map(str::to_string)
}

/// 脚本里的内存上限只看行首是 `java` 的那一行（与 `verify_run::launch_command` 同一认法）。
/// 首启自装那档的 `start.bat` 里 `java -Xmx1G … --installServer` 挂在 `if` 之后，那是安装器的
/// 堆不是服务端的堆，照着报会把人带偏 ⇒ 这一档宁可不报
fn script_xmx(text: &str) -> Option<String> {
    text.lines().map(str::trim).find_map(|line| {
        let rest = line.strip_prefix("java")?;
        if !(rest.is_empty() || rest.starts_with(' ') || rest.starts_with('\t')) {
            return None;
        }
        xmx_token(rest.split_whitespace())
    })
}

/// `key=value` 里的布尔项：键 trim 后比对，值不是 `true`/`false`（被人手改成别的）就当没有
fn prop_bool(text: &str, key: &str) -> Option<bool> {
    text.lines().find_map(|line| {
        let (k, v) = line.split_once('=')?;
        (k.trim() == key)
            .then(|| v.trim().parse::<bool>().ok())
            .flatten()
    })
}

/// zip 原文与副本文件两条读路共用的取数（`read` 给不出内容就是这一处没有）
fn hints_with(mut read: impl FnMut(&str) -> Option<String>) -> PackHints {
    let mut out = PackHints::default();
    for name in HINT_SCRIPTS {
        if out.xmx.is_some() {
            break;
        }
        let Some(text) = read(name) else { continue };
        // 参数文件整份就是一行 JVM 参数，没有 `java` 前缀可认
        out.xmx = if name == "user_jvm_args.txt" {
            xmx_token(text.split_whitespace())
        } else {
            script_xmx(&text)
        };
    }
    out.online_mode = read("server.properties").and_then(|t| prop_bool(&t, "online-mode"));
    out
}

/// 交付包里的验证参数线索。读的是 zip 原文 ⇒ 还没解包（Ready 那一档）也能给
pub fn pack_hints(zip_path: &Path) -> PackHints {
    let Ok(file) = File::open(zip_path) else {
        return PackHints::default();
    };
    let mut archive = match zip::ZipArchive::new(file) {
        Ok(a) => a,
        Err(_) => return PackHints::default(),
    };
    hints_with(|name| {
        let mut text = String::new();
        archive
            .by_name(name)
            .ok()?
            .read_to_string(&mut text)
            .ok()?;
        Some(text)
    })
}

/// 副本里的验证参数线索：跑过一轮之后以这份为准（端口闸门动的也是这份）
pub fn copy_hints(copy_dir: &Path) -> PackHints {
    hints_with(|name| fs::read_to_string(copy_dir.join(name)).ok())
}

/// 副本根里那份加载器安装器 jar（`forge-installer.jar` / `*installer*.jar`）：
/// 未本机安装那一档，首启自装靠的就是它，验证的「加载器」这一步也直接跑它
pub fn installer_jar_in(copy_dir: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(copy_dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
        if name.ends_with(".jar") && name.contains("installer") {
            return Some(path);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use zip::write::SimpleFileOptions;

    /// 一个只在测试里活的副本工作区：`{tmp}\sideshift-verify-copy-{uuid}`
    struct Space {
        cache: PathBuf,
        zip: PathBuf,
    }

    impl Space {
        fn new(entries: &[(&str, &[u8])]) -> Self {
            let root = std::env::temp_dir().join(format!(
                "sideshift-verify-copy-{}",
                uuid::Uuid::new_v4()
            ));
            let cache = root.join("cache");
            fs::create_dir_all(&cache).unwrap();
            let zip = root.join("pack.zip");
            write_zip(&zip, entries);
            Self { cache, zip }
        }

        /// 同名路径换内容 = 重新导出了交付包（同路径原地覆盖那一档）
        fn rewrite_zip(&self, entries: &[(&str, &[u8])]) {
            write_zip(&self.zip, entries);
        }

        fn drop_(self) {
            let root = self.cache.parent().unwrap();
            let _ = fs::remove_dir_all(root);
        }
    }

    fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let mut w = zip::ZipWriter::new(File::create(path).unwrap());
        let opts = SimpleFileOptions::default();
        for (name, bytes) in entries {
            w.start_file(*name, opts).unwrap();
            w.write_all(bytes).unwrap();
        }
        w.finish().unwrap();
    }

    const PACK: &[(&str, &[u8])] = &[
        ("start.bat", b"@echo off\r\njava -Xmx1G -jar server.jar nogui\r\n"),
        ("eula.txt", b"# head comment\r\neula=true\r\n"),
        ("server.properties", b"#Minecraft server properties\nserver-port=25570\n"),
        ("mods/a.jar", b"aaa"),
        ("mods/b.jar", b"bbb"),
        ("mods/README.txt", b"not a mod"),
    ];

    /// 首次解包：文件数与字节来自实解，哨兵落盘，mods 清单只认 `.jar`
    #[test]
    fn first_prepare_extracts_and_indexes_mods() {
        let s = Space::new(PACK);
        let p = prepare(&s.cache, "task-1", &s.zip).unwrap();
        assert!(!p.reused);
        assert_eq!(p.files, PACK.len());
        assert_eq!(p.bytes, PACK.iter().map(|(_, b)| b.len() as u64).sum::<u64>());
        assert!(p.dir.join(MANIFEST).is_file(), "哨兵必须落盘，否则下次无法判断能不能复用");
        assert_eq!(p.dir, copy_dir(&s.cache, "task-1").unwrap());

        let mods = list_mods(&s.cache, "task-1").unwrap();
        assert_eq!(
            mods.iter().map(|m| m.file.as_str()).collect::<Vec<_>>(),
            vec!["a.jar", "b.jar"],
            "README.txt 不是模组"
        );
        assert_eq!(mod_counts(&p.dir), (2, 0));
        assert_eq!(server_port(&p.dir), Some(25570), "注释行不能把 server-port 顶掉");
        assert_eq!(eula_agreed(&s.zip), Some(true));
        s.drop_();
    }

    /// 同一个包再验一次：整份复用，上次在副本里做的启停留着（那是用户改的，不是包里的）
    #[test]
    fn same_pack_reuses_the_copy() {
        let s = Space::new(PACK);
        prepare(&s.cache, "task-2", &s.zip).unwrap();
        set_mod_enabled(&s.cache, "task-2", "a.jar", false).unwrap();
        let again = prepare(&s.cache, "task-2", &s.zip).unwrap();
        assert!(again.reused);
        assert_eq!(again.files, PACK.len());
        let mods = list_mods(&s.cache, "task-2").unwrap();
        assert_eq!(mods[0].file, "a.jar.disabled", "复用必须保住上次的停用");
        assert_eq!(mod_counts(&again.dir), (2, 1));
        s.drop_();
    }

    /// 包被重新导出过（同路径换字节）：整份重解，上次启停跟着清零——留着旧字节是在验一份不存在的包
    #[test]
    fn repacked_zip_rebuilds_from_scratch() {
        let s = Space::new(PACK);
        prepare(&s.cache, "task-3", &s.zip).unwrap();
        set_mod_enabled(&s.cache, "task-3", "a.jar", false).unwrap();

        let mut fresh: Vec<(&str, &[u8])> = PACK.to_vec();
        fresh.push(("mods/c.jar", b"ccc"));
        s.rewrite_zip(&fresh);
        let p = prepare(&s.cache, "task-3", &s.zip).unwrap();
        assert!(!p.reused, "字节数变了就该重解");
        let mods = list_mods(&s.cache, "task-3").unwrap();
        assert_eq!(
            mods.iter().map(|m| m.file.as_str()).collect::<Vec<_>>(),
            vec!["a.jar", "b.jar", "c.jar"],
            "重解后 a.jar 回到启用态，且新那枚进清单"
        );
        s.drop_();
    }

    /// 启停往返：改名加/削 `.disabled`；目标同名已存在时报错且一个文件都不动
    #[test]
    fn toggling_renames_and_refuses_to_overwrite() {
        let s = Space::new(PACK);
        prepare(&s.cache, "task-4", &s.zip).unwrap();
        let off = set_mod_enabled(&s.cache, "task-4", "a.jar", false).unwrap();
        assert_eq!(off.file, "a.jar.disabled");
        assert_eq!(off.name, "a.jar");
        assert!(!off.enabled);
        // 已经是目标态：原样返回，不报错（界面重复点一次不该红）
        assert_eq!(set_mod_enabled(&s.cache, "task-4", "a.jar.disabled", false).unwrap(), off);
        let on = set_mod_enabled(&s.cache, "task-4", "a.jar.disabled", true).unwrap();
        assert_eq!(on.file, "a.jar");
        assert!(on.enabled);

        // 同名两份并存（包里自带一枚 .jar.disabled）：改名会吃掉另一枚 ⇒ 拒绝
        let dir = copy_dir(&s.cache, "task-4").unwrap();
        fs::write(dir.join(MODS_DIR).join("b.jar.disabled"), b"dup").unwrap();
        let err = set_mod_enabled(&s.cache, "task-4", "b.jar", false).unwrap_err();
        assert!(err.contains("b.jar.disabled"), "{err}");
        assert!(dir.join(MODS_DIR).join("b.jar").is_file(), "拒绝之后两边都还在");
        assert!(dir.join(MODS_DIR).join("b.jar.disabled").is_file());

        // 不是模组的名字、带路径的名字：一律拒，且不动盘
        assert!(set_mod_enabled(&s.cache, "task-4", "README.txt", false).is_err());
        assert!(set_mod_enabled(&s.cache, "task-4", "../a.jar", false).is_err());
        s.drop_();
    }

    /// 端口闸门只动 `server-port` 那一行：其余配置原样留着，读回来的是新值。
    /// 交付包压根没带这份配置时新建一份只带这一条的（服务端起来后会自己补全默认项）
    #[test]
    fn set_server_port_rewrites_only_that_line() {
        let s = Space::new(&[
            ("server.properties", b"server-port=25565\nmotd=hello\n"),
            ("mods/a.jar", b"a"),
        ]);
        let prepared = prepare(&s.cache, "task-port", &s.zip).unwrap();
        assert_eq!(server_port(&prepared.dir), Some(25565));
        set_server_port(&prepared.dir, 51820).unwrap();
        assert_eq!(server_port(&prepared.dir), Some(51820));
        let text = fs::read_to_string(prepared.dir.join("server.properties")).unwrap();
        assert!(text.contains("motd=hello"), "别的配置不能被吞：{text}");
        assert!(!text.contains("25565"), "旧端口不该还留在文件里：{text}");
        s.drop_();

        let bare = Space::new(&[("mods/a.jar", b"a")]);
        let p = prepare(&bare.cache, "task-bare", &bare.zip).unwrap();
        assert_eq!(server_port(&p.dir), None, "包里没有这份配置");
        set_server_port(&p.dir, 25599).unwrap();
        assert_eq!(server_port(&p.dir), Some(25599));
        bare.drop_();
    }

    /// 占用判定与真实监听句柄一致：绑住的口判为占用，放开后同一个口应当空出来
    #[test]
    fn port_probe_agrees_with_a_real_listener() {
        use std::net::{Ipv4Addr, TcpListener};
        let held = TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)).unwrap();
        let port = held.local_addr().unwrap().port();
        assert!(port_in_use(port), "已经绑上的端口应判为占用");
        drop(held);
        assert!(!port_in_use(port), "放开之后同一个口应当空出来");
        assert!(free_port().is_some(), "系统应当能给出口空闲端口");
    }

    /// 非法任务 id 不拼路径（这里的每个操作都含递归删除）
    #[test]
    fn refuses_unsafe_task_ids() {
        let s = Space::new(PACK);
        let long = "x".repeat(65);
        for bad in ["", "..", "a/b", "a\\b", "../x", long.as_str()] {
            assert!(copy_dir(&s.cache, bad).is_err(), "{bad} 竟被当成目录名");
            assert!(prepare(&s.cache, bad, &s.zip).is_err());
            assert!(teardown(&s.cache, bad).is_err());
        }
        s.drop_();
    }

    /// 包不在 / 不是文件 / 没这个条目：各自实话报错，且不留下半个副本
    #[test]
    fn missing_or_foreign_zip_reports_plainly() {
        let s = Space::new(PACK);
        let gone = s.cache.join("nope.zip");
        let err = prepare(&s.cache, "task-5", &gone).unwrap_err();
        assert!(err.contains("交付包不在了"), "{err}");
        assert!(!copy_dir(&s.cache, "task-5").unwrap().exists());

        let not_a_file = s.cache.clone();
        assert!(prepare(&s.cache, "task-5", &not_a_file)
            .unwrap_err()
            .contains("不是文件"));
        s.drop_();
    }

    /// 启动清扫：注册表里没有的副本目录收走，全空时连 verify 根一起收
    #[test]
    fn sweep_removes_orphans_only() {
        let s = Space::new(PACK);
        for id in ["keep-1", "gone-1", "gone-2"] {
            prepare(&s.cache, id, &s.zip).unwrap();
        }
        sweep_orphans(&s.cache, |name| name == "keep-1");
        assert!(copy_dir(&s.cache, "keep-1").unwrap().is_dir());
        assert!(!copy_dir(&s.cache, "gone-1").unwrap().exists());
        assert!(!copy_dir(&s.cache, "gone-2").unwrap().exists());

        // 反复点清理：副本已经没了也算成功
        assert!(teardown(&s.cache, "gone-1").is_ok());
        sweep_orphans(&s.cache, |_| false);
        assert!(
            !s.cache.join(CACHE_VERIFY_DIR).exists(),
            "verify 根空了就该一起收掉，别在设置页永远露一个空壳"
        );
        s.drop_();
    }

    /// 首启自装那一档的判据：副本根里带 installer 字样的 jar；eula 没同意/没有各自一档
    #[test]
    fn finds_installer_jar_and_reads_eula() {
        let s = Space::new(&[
            ("forge-1.20.1-47.2.0-installer.jar", b"x"),
            ("server.properties", b"server-port=25565\n"),
        ]);
        let p = prepare(&s.cache, "task-6", &s.zip).unwrap();
        assert_eq!(
            installer_jar_in(&p.dir).and_then(|x| x.file_name().map(|n| n.to_string_lossy().to_string())),
            Some("forge-1.20.1-47.2.0-installer.jar".to_string())
        );
        assert_eq!(eula_agreed(&s.zip), None, "包里没有 eula.txt");
        assert_eq!(server_port(&p.dir), Some(25565));

        let no = Space::new(&[("eula.txt", b"# c\r\neula=false\r\n")]);
        assert_eq!(eula_agreed(&no.zip), Some(false));
        let yes = Space::new(&[("eula.txt", b"eula = true\r\n")]);
        assert_eq!(eula_agreed(&yes.zip), Some(true), "等号两边留空格也认");
        s.drop_();
        no.drop_();
        yes.drop_();
    }

    /// 验证参数线索：内存上限取参数文件或行首那句 `java`，安装器那句不算；`online-mode` 按键认
    #[test]
    fn hints_read_pack_and_copy() {
        // 新式加载器：启动脚本靠 `@user_jvm_args.txt`，内存上限写在参数文件里
        let s = Space::new(&[
            (
                "start.bat",
                b"@echo off\r\ncd /d \"%~dp0\"\r\njava @user_jvm_args.txt @libraries/x-win_args.txt %*\r\n",
            ),
            ("user_jvm_args.txt", b"-Xmx4096M -XX:+UseG1GC\n"),
            ("server.properties", b"online-mode = false\nserver-port=25565\n"),
        ]);
        let by_pack = pack_hints(&s.zip);
        assert_eq!(
            by_pack.xmx.as_deref(),
            Some("-Xmx4096M"),
            "参数文件里那串优先，脚本行首没有 -Xmx 就取它"
        );
        assert_eq!(by_pack.online_mode, Some(false), "等号两边留空格也认");
        let p = prepare(&s.cache, "task-7", &s.zip).unwrap();
        assert_eq!(copy_hints(&p.dir), by_pack, "同一份内容，副本读法要与 zip 读法同结论");
        s.drop_();

        // Fabric / 老 Forge：内存上限直接写在脚本命令行上
        let legacy = Space::new(&[(
            "start.sh",
            b"#!/usr/bin/env bash\njava -Xmx2G -jar server.jar nogui\n",
        )]);
        let h = pack_hints(&legacy.zip);
        assert_eq!(h.xmx.as_deref(), Some("-Xmx2G"));
        assert_eq!(
            h.online_mode, None,
            "包里没有 server.properties 就不报，别替服务端补默认值"
        );
        legacy.drop_();

        // 首启自装那一档：脚本里唯一的 -Xmx 属于安装器，报出来会把人带偏 ⇒ 不报
        let first_boot = Space::new(&[(
            "start.bat",
            b"@echo off\r\nif not exist run.bat java -Xmx1G -jar forge-installer.jar --installServer\r\nif exist run.bat (call run.bat nogui)\r\n",
        )]);
        assert_eq!(pack_hints(&first_boot.zip).xmx, None);
        first_boot.drop_();
    }
}
