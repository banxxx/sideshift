use super::*;

/// 已是压缩格式的后缀：jar/zip 本体就是 deflate，png/ogg 是有损压缩，
/// 再压一遍只烧 CPU 不省体积——单线程 Deflate 压几百 MB mod 就是「构建特别慢」的全部原因
const STORED_EXTS: &[&str] = &[
    "jar", "zip", "png", "jpg", "jpeg", "webp", "gif", "ogg", "mp3", "mp4", "webm", "7z", "gz",
    "bz2", "xz", "zst", "woff", "woff2", "tga", "dds", "bundled",
];

pub(super) fn stored_for(rel: &str) -> bool {
    match rel.rsplit_once('.') {
        Some((_, ext)) => STORED_EXTS.contains(&ext.to_ascii_lowercase().as_str()),
        None => false,
    }
}

/// 逐条目写入：jar 这类已压缩内容走 Stored（ deflate 再压一遍不省体积只烧时间），
/// 文本走 Deflated；每个顶层目录写完发一条 Group 事件
pub(super) fn write_zip(
    zip: &mut zip::ZipWriter<BufWriter<File>>,
    plan: &[Planned],
    on_event: &mut dyn FnMut(&BuildEvent),
) -> Result<(), BuilderError> {
    let mut groups = group_totals(plan);
    for p in plan {
        let mut opts = SimpleFileOptions::default().compression_method(if stored_for(&p.rel) {
            CompressionMethod::Stored
        } else {
            CompressionMethod::Deflated
        });
        // zip 默认不携带 unix 权限（解压后 644），shell 脚本需补执行位
        if p.exec {
            opts = opts.unix_permissions(0o755);
        }
        zip.start_file(&p.rel, opts)?;
        let mut f = File::open(&p.path)?;
        std::io::copy(&mut f, zip)?;
        on_event(&BuildEvent::File { group: p.group.clone(), bytes: p.size });
        let Some(g) = groups.iter_mut().find(|g| g.label == p.group) else {
            continue;
        };
        g.done += 1;
        if g.done == g.files && !g.flushed {
            g.flushed = true;
            on_event(&BuildEvent::Group {
                label: g.label.clone(),
                files: g.files,
                bytes: g.bytes,
            });
        }
    }
    Ok(())
}

/// 待写入条目：绝对路径 + zip 内相对路径 + 大小 + 归属顶层分组
pub(super) struct Planned {
    path: PathBuf,
    rel: String,
    pub(super) size: u64,
    group: String,
    /// shell 脚本：zip 里要带 755 执行位
    exec: bool,
}

#[derive(Clone)]
struct GroupAcc {
    label: String,
    files: usize,
    bytes: u64,
    done: usize,
    flushed: bool,
}

/// staging 第一层目录即一个分组（mods/ 说「模组」，包根散件说「根文件」）——
/// 与取件阶段的分目录日志同一套口径
pub(super) fn group_of(rel: &str) -> String {
    match rel.split_once('/') {
        Some((head, _)) if head.eq_ignore_ascii_case("mods") => "模组".to_string(),
        Some((head, _)) => head.to_string(),
        None => "根文件".to_string(),
    }
}

fn group_totals(plan: &[Planned]) -> Vec<GroupAcc> {
    let mut out: Vec<GroupAcc> = Vec::new();
    for p in plan {
        match out.iter_mut().find(|g| g.label == p.group) {
            Some(g) => {
                g.files += 1;
                g.bytes += p.size;
            }
            None => out.push(GroupAcc {
                label: p.group.clone(),
                files: 1,
                bytes: p.size,
                done: 0,
                flushed: false,
            }),
        }
    }
    out
}

/// 递归收集待打包条目（排序保证目录顺序稳定，日志与产物可复现）
pub(super) fn collect(root: &Path, dir: &Path, out: &mut Vec<Planned>) -> Result<(), BuilderError> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)?.filter_map(|e| e.ok().map(|e| e.path())).collect();
    entries.sort();
    for path in entries {
        let meta = std::fs::metadata(&path)?;
        if meta.is_dir() {
            collect(root, &path, out)?;
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .map_err(|e| BuilderError::Io(std::io::Error::other(e)))?
            .to_string_lossy()
            .replace('\\', "/");
        out.push(Planned {
            group: group_of(&rel),
            exec: rel.to_lowercase().ends_with(".sh"),
            size: meta.len(),
            path,
            rel,
        });
    }
    Ok(())
}
