//! 任务引擎内部的小工具：时间戳、人读体积、列表简述、落位文件名。

use std::collections::HashSet;

/// 人读体积（日志文案用；1MB = 1000KB 口径，与前端 formatSize 一致）
pub fn fmt_size(bytes: u64) -> String {
    const KB: f64 = 1000.0;
    let b = bytes as f64;
    if b >= KB * KB * KB {
        format!("{:.2} GB", b / KB / KB / KB)
    } else if b >= KB * KB {
        format!("{:.1} MB", b / KB / KB)
    } else if b >= KB {
        format!("{:.0} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

/// 列表简述：最多 8 项，超出补「等 N 个」
pub fn brief_list(items: &[String]) -> String {
    if items.len() <= 8 {
        return items.join("、");
    }
    format!("{}…等 {} 个", items[..8].join("、"), items.len())
}

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

pub fn now_hms() -> String {
    chrono::Local::now().format("%H:%M:%S").to_string()
}

pub fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '-' })
        .collect()
}

/// mods/ 唯一落位名：同名 jar（来自不同目录）第二份起追加方案 id，防静默覆盖
pub fn unique_mod_name(used: &mut HashSet<String>, name: &str, id: &str) -> String {
    if used.insert(name.to_lowercase()) {
        return name.to_string();
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), e.to_string()),
        _ => (name.to_string(), "jar".to_string()),
    };
    let sid = sanitize(id);
    let mut candidate = format!("{stem}-{sid}.{ext}");
    let mut n = 2;
    while !used.insert(candidate.to_lowercase()) {
        candidate = format!("{stem}-{sid}-{n}.{ext}");
        n += 1;
    }
    candidate
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_output_names_avoid_overwrite() {
        let mut used = HashSet::new();
        assert_eq!(unique_mod_name(&mut used, "a.jar", "m1"), "a.jar");
        assert_eq!(unique_mod_name(&mut used, "a.jar", "m2"), "a-m2.jar");
        assert_eq!(unique_mod_name(&mut used, "a-m2.jar", "m3"), "a-m2-m3.jar");
    }
}
