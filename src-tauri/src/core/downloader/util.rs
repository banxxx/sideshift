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
    Some(cache_dir.join("files").join(key).join(&item.file_name))
}

/// 缓存复用前提：声明了 sha1 就必须与内容一致（不一致视作缓存损坏，重新获取）
pub fn verify_cache_for(item: &ItemSpec, cache: &Path) -> bool {
    let Some(expect) = &item.sha1 else {
        return true;
    };
    let bytes = std::fs::read(cache).unwrap_or_default();
    sha1_hex(&bytes).eq_ignore_ascii_case(expect)
}
