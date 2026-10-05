//! 取件用的小件：URL 编码、哈希与缓存键、版本比较、加载器类别名。

use std::path::{Path, PathBuf};

use crate::models::LoaderKind;
use super::types::{Fetch, ItemSpec};

pub fn loader_cat(loader: LoaderKind) -> &'static str {
    match loader {
        LoaderKind::Fabric => "fabric",
        LoaderKind::Forge => "forge",
        LoaderKind::NeoForge => "neoforge",
    }
}

/// 极简 URL 编码（查询参数场景）
pub fn urlencoding(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// 下载缓存的根子目录名：`{cache}\files\{键}\{文件名}`。字面量在 `core::data_root`（那张表
/// 同时是卸载壳的删除清单），这里把它接进本模块的命名空间，清理侧仍引这一个名字
pub use crate::core::data_root::CACHE_FILES_DIR;
/// 半成品后缀：`{文件名}.part{尝试序号}`（写侧是 client 的 `temp_path`，判侧是 `is_partial_name`）
pub(crate) const PART_MARKER: &str = ".part";

/// 是不是半截下载的临时文件。用 contains 而不是 ends_with：后缀带尝试序号
pub fn is_partial_name(name: &str) -> bool {
    name.contains(PART_MARKER)
}

/// 命中复用时把「最后一次使用时间」写在文件自己身上。
///
/// 为什么是 mtime 而不是另立一份时间戳索引：mtime 跟着文件走，用户手工删缓存、
/// 换目录、清半个盘都不会让索引失配（那种索引一旦对不上，过期判据就集体失效）。
/// 写失败一律吞掉——缓存清理是个次要功能，没有能力把转换弄失败。
pub fn mark_used(path: &Path) {
    if let Ok(f) = std::fs::OpenOptions::new().write(true).open(path) {
        let _ = f.set_modified(std::time::SystemTime::now());
    }
}

/// sha1 → 小写 hex（mrpack/Modrinth/maven 的校验值口径均为 hex）
pub fn sha1_hex(bytes: &[u8]) -> String {
    use sha1::{Digest, Sha1};
    let mut h = Sha1::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// 简易 FNV-1a 哈希（无 sha1 时的缓存键；仅防碰撞用，非安全场景）
fn url_hash(url: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in url.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("h{h:016x}")
}

/// 数字段版本比较（"0.15.3" vs "0.9.2"）
pub fn version_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let pa: Vec<u64> = a.split('.').filter_map(|s| s.parse().ok()).collect();
    let pb: Vec<u64> = b.split('.').filter_map(|s| s.parse().ok()).collect();
    pa.cmp(&pb)
}

pub fn cache_path_for(cache_dir: &Path, item: &ItemSpec) -> Option<PathBuf> {
    let key = match &item.sha1 {
        Some(s) => s.clone(),
        None => match &item.fetch {
            Fetch::Url(_) => url_hash(&item.source_key()),
            _ => return None,
        },
    };
    Some(cache_dir.join(CACHE_FILES_DIR).join(key).join(&item.file_name))
}

/// 缓存复用前提：声明了 sha1 就必须与内容一致（不一致视作缓存损坏，重新获取）
pub fn verify_cache_for(item: &ItemSpec, cache: &Path) -> bool {
    let Some(expect) = &item.sha1 else {
        return true;
    };
    let bytes = std::fs::read(cache).unwrap_or_default();
    sha1_hex(&bytes).eq_ignore_ascii_case(expect)
}
