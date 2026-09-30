//! 离线批量取件（download_all 的 harvest 通道）：纯复制分片与单归档单遍解包。
//! 约束：一个分片一个归档句柄（中央目录只解析一次）、条目流式直写目标文件。

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use super::types::{DownloadError, Fetch, FetchSource, ItemOutcome, ItemSpec, OnDone};
use super::util::mark_used;

/// 一次纯复制型任务：缓存命中或本地 jar → 目标路径
#[derive(Clone)]
pub struct CopyJob {
    pub src: PathBuf,
    /// 区分日志动词与「是否算联网件」口径
    pub from_cache: bool,
    pub item: ItemSpec,
}

/// 离线分片：一批复制，或某个归档的一个条目分片（分片内共用一个包句柄）
pub enum OfflineJob {
    Copy(Vec<CopyJob>),
    Extract(PathBuf, Vec<ItemSpec>),
}

/// 复制分片大小：小文件落盘在 Windows 上按次收费（含杀软拦截），分片才能并行
pub const COPY_CHUNK: usize = 64;

pub fn report(done: &AtomicUsize, total: usize, on_done: &OnDone, outcome: ItemOutcome) {
    let n = done.fetch_add(1, Ordering::Relaxed) + 1;
    on_done(n, total, &outcome);
}

/// 用户取消或任一分片已失败：本分片就地收摊
fn aborted(cancel: &AtomicBool, stop: &AtomicBool) -> bool {
    cancel.load(Ordering::Relaxed) || stop.load(Ordering::Relaxed)
}

/// 分片数：小批量单线程更省（每片都要解析一次中央目录），大批量才值得并行
pub fn shard_count(n: usize, concurrency: usize) -> usize {
    if n <= 16 {
        return 1;
    }
    concurrency.clamp(1, 8).min(n.div_ceil(32)).max(1)
}

pub fn run_copies(
    list: Vec<CopyJob>,
    cancel: &AtomicBool,
    stop: &AtomicBool,
    done: &AtomicUsize,
    total: usize,
    on_done: &OnDone,
) -> Result<(), DownloadError> {
    for job in list {
        if aborted(cancel, stop) {
            return Ok(());
        }
        let CopyJob {
            src,
            from_cache,
            item,
        } = job;
        let bytes = copy_to_dest(&src, &item)?;
        // 复用即续命：缓存清理按 mtime 判「最后一次使用」，不刷新就会把常用包判成过期
        if from_cache {
            mark_used(&src);
        }
        let source = match &item.fetch {
            Fetch::Url(_) => FetchSource::Network,
            Fetch::ZipEntry { .. } => FetchSource::Pack,
            Fetch::Local(_) => FetchSource::Local,
        };
        report(
            done,
            total,
            on_done,
            ItemOutcome {
                file_name: item.file_name.clone(),
                source,
                cached: from_cache,
                bytes,
                retries: 0,
                dest: item.dest.clone(),
            },
        );
    }
    Ok(())
}

fn copy_to_dest(src: &Path, item: &ItemSpec) -> Result<u64, DownloadError> {
    if let Some(parent) = item.dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(src, &item.dest).map_err(|e| DownloadError::Failed {
        file_name: item.file_name.clone(),
        attempts: 0,
        cause: format!("{e}（源={} · dest={}）", src.display(), item.dest.display()),
    })
}

/// 单归档分片：开包与中央目录解析各一次，条目按序流式写入目标
pub fn run_extract(
    archive: &Path,
    list: Vec<ItemSpec>,
    cancel: &AtomicBool,
    stop: &AtomicBool,
    done: &AtomicUsize,
    total: usize,
    on_done: &OnDone,
) -> Result<(), DownloadError> {
    let open = |cause: String| DownloadError::Failed {
        file_name: archive
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| archive.display().to_string()),
        attempts: 0,
        cause,
    };
    let f = File::open(archive).map_err(|e| open(format!("打开整合包失败：{e}")))?;
    let mut z = zip::ZipArchive::new(f).map_err(|e| open(format!("zip 读取失败：{e}")))?;

    for item in list {
        if aborted(cancel, stop) {
            return Ok(());
        }
        let entry = match &item.fetch {
            Fetch::ZipEntry { entry, .. } => entry.clone(),
            _ => return Err(open("非包内条目分片".to_string())),
        };
        let mut rf = z
            .by_name(&entry)
            .map_err(|e| open(format!("包内条目缺失：{e}（{entry}）")))?;
        let bytes = write_entry(&mut rf, &item)?;
        report(
            done,
            total,
            on_done,
            ItemOutcome {
                file_name: item.file_name.clone(),
                source: FetchSource::Pack,
                cached: false,
                bytes,
                retries: 0,
                dest: item.dest.clone(),
            },
        );
    }
    Ok(())
}

/// 条目 → 目标文件流式落盘，边写边算 sha1（声明了才校验，不匹配则删掉半成品）
fn write_entry<R: std::io::Read>(rf: &mut R, item: &ItemSpec) -> Result<u64, DownloadError> {
    use sha1::{Digest, Sha1};
    if let Some(parent) = item.dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut out = File::create(&item.dest).map_err(|e| DownloadError::Failed {
        file_name: item.file_name.clone(),
        attempts: 0,
        cause: format!("{e}（dest={}）", item.dest.display()),
    })?;
    let expect = item.sha1.clone();
    let mut hasher = expect.as_ref().map(|_| Sha1::new());
    let mut buf = [0u8; 64 * 1024];
    let mut written = 0u64;
    loop {
        let k = rf.read(&mut buf)?;
        if k == 0 {
            break;
        }
        if let Some(h) = hasher.as_mut() {
            h.update(&buf[..k]);
        }
        std::io::Write::write_all(&mut out, &buf[..k])?;
        written += k as u64;
    }
    drop(out);
    if let (Some(h), Some(expect)) = (hasher, &expect) {
        let got: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
        if !got.eq_ignore_ascii_case(expect) {
            let _ = std::fs::remove_file(&item.dest);
            return Err(DownloadError::Failed {
                file_name: item.file_name.clone(),
                attempts: 0,
                cause: format!("sha1 校验不一致（期望 {expect} · 实际 {got}）"),
            });
        }
    }
    Ok(written)
}
