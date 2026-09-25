//! `Downloader` 本体：并发取件总调度、联网流式取回与重试，以及 GET/POST/HEAD 三个 HTTP 原语。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use futures::stream::{self, StreamExt};
use reqwest::Client;
use serde_json::Value;

use super::offline::{self, CopyJob, OfflineJob, COPY_CHUNK};
use super::source;
use super::types::{
    DownloadError, Fetch, FetchSource, ItemOutcome, ItemSpec, OnDone, OnTransfer, TransferProgress,
};
use super::util::{cache_path_for, mark_used, verify_cache_for, PART_MARKER};
use crate::models::DownloadSource;

const USER_AGENT: &str = "SideShift/0.1 (desktop pack converter)";
const RETRIES: u32 = 3;

/// 单次下载尝试的失败分类
enum Attempt {
    /// 换个时间再试可能成功（连接重置、429/5xx）：退避后重试
    Retry(String),
    /// 这个源给的内容不对（sha1 对不上）：换一个源试；已是最后一个源则等同失败
    BadHost(String),
    /// 重试也不会变好（磁盘写不进）：立即结束本条目
    Fatal(DownloadError),
}

/// 落盘类失败：带上第几次尝试与出问题的路径，日志里能一眼看出是网络还是磁盘
fn io_err(item: &ItemSpec, attempt: u32, e: &std::io::Error, path: &Path) -> Attempt {
    Attempt::Fatal(DownloadError::Failed {
        file_name: item.file_name.clone(),
        attempts: attempt.saturating_sub(1),
        cause: format!("{e}（{}）", path.display()),
    })
}

/// 尝试专属临时名：并发重试不会互相踩到同一个半成品
fn temp_path(target: &Path, attempt: u32) -> PathBuf {
    let stem = target
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("download");
    target.with_file_name(format!("{stem}{PART_MARKER}{attempt}"))
}

pub struct Downloader {
    pub client: Client,
    cache_dir: PathBuf,
    concurrency: usize,
    transfer: Option<Arc<OnTransfer>>,
    /// 下载源档位：只影响「哪些 URL 先试镜像」，不改校验锚点（见 `source` 模块头）
    source: DownloadSource,
    /// CurseForge 的 `x-api-key`（来自设置）。`None` = 用户没配：那一侧的查询当场报「去配置」，
    /// 不发请求。它只是被搬运到请求头里，任何日志与错误文案都不许带上它的值
    cf_key: Option<String>,
}

impl Downloader {
    pub fn new(cache_dir: PathBuf, concurrency: usize) -> Self {
        let client = Client::builder()
            .user_agent(USER_AGENT)
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .expect("reqwest client");
        Self {
            client,
            cache_dir,
            concurrency: concurrency.clamp(1, 16),
            transfer: None,
            source: DownloadSource::Official,
            cf_key: None,
        }
    }

    /// 挂上字节进度出口：流水线据此渲染「联网下载中」实时条
    pub fn with_transfer(mut self, f: Arc<OnTransfer>) -> Self {
        self.transfer = Some(f);
        self
    }

    /// 挂上设置里的下载源：未调用即官方源（保持默认行为）
    pub fn with_source(mut self, source: DownloadSource) -> Self {
        self.source = source;
        self
    }

    /// 挂上设置里的 CurseForge Key：空串按「没配」处理（与 `AppSettings::normalized` 同一口径，
    /// 两处都判一次，免得有人绕过设置直接构造）
    pub fn with_curseforge_key(mut self, key: Option<String>) -> Self {
        self.cf_key = key.map(|k| k.trim().to_string()).filter(|k| !k.is_empty());
        self
    }

    /// 缓存路径；`None` = 该项不落缓存。URL 项按 sha1（缺省按 URL 哈希，Modrinth 坐标与内容一一对应）
    /// 缓存；包内 / 本地项只有在声明 sha1 时才缓存——否则源文件被替换后缓存键不变，会静默复用旧内容
    fn cache_path_opt(&self, item: &ItemSpec) -> Option<PathBuf> {
        cache_path_for(&self.cache_dir, item)
    }

    /// 并发取件（各自带 dest 绝对路径）；cancel 置位后未开始的文件直接跳过；
    /// 每完成一个文件回调 (done, total, 本条结果)
    ///
    /// 两条通道：**联网项**走 reqwest 并发 + 重试；**离线项**（整合包条目 / 本地 jar / 缓存命中）
    /// 走批量单遍解出——一个分片一个归档句柄（中央目录只解析一次）、条目流式直写目标文件，
    /// 不再「每个文件重开一次包 + 整份读进内存 + 落两次盘」。保留目录动辄几百个小文件，
    /// 旧写法的时间全花在重复开包上，而不是字节量。
    pub async fn download_all(
        &self,
        items: Vec<ItemSpec>,
        cancel: Arc<AtomicBool>,
        on_done: impl Fn(usize, usize, &ItemOutcome) + Send + Sync + 'static,
    ) -> Result<(), DownloadError> {
        let total = items.len();
        let done = Arc::new(AtomicUsize::new(0));
        let on_done: Arc<OnDone> = Arc::new(on_done);
        let first_err: Arc<std::sync::Mutex<Option<DownloadError>>> =
            Arc::new(std::sync::Mutex::new(None));

        let (offline, net): (Vec<ItemSpec>, Vec<ItemSpec>) = items
            .into_iter()
            .partition(|i| !matches!(i.fetch, Fetch::Url(_)));
        if !offline.is_empty() {
            self.harvest_offline(offline, &cancel, &done, total, &on_done, &first_err)
                .await;
        }

        stream::iter(net)
            .map(|item| {
                let done = done.clone();
                let on_done = on_done.clone();
                let first_err = first_err.clone();
                let cancel = cancel.clone();
                async move {
                    if cancel.load(Ordering::Relaxed) || first_err.lock().unwrap().is_some() {
                        return; // 已取消或已有失败：其余任务直接跳过
                    }
                    let r = self.download_one(&item).await;
                    match r {
                        Ok(outcome) => offline::report(done.as_ref(), total, &*on_done, outcome),
                        Err(e) => {
                            let mut g = first_err.lock().unwrap();
                            if g.is_none() {
                                *g = Some(e);
                            }
                        }
                    }
                }
            })
            .buffer_unordered(self.concurrency)
            .collect::<()>()
            .await;

        if let Some(e) = first_err.lock().unwrap().take() {
            return Err(e);
        }
        Ok(())
    }

    /// 离线取件总调度：分拣出「纯复制」（缓存 / 本地）与「包内条目」（按归档分组），
    /// 再切成若干分片丢进 blocking 池——复制分片让多个小文件并行落盘，
    /// 解包分片让 inflate 这种 CPU 活并行，同时把中央目录解析次数压到「每分片一次」。
    async fn harvest_offline(
        &self,
        items: Vec<ItemSpec>,
        cancel: &Arc<AtomicBool>,
        done: &Arc<AtomicUsize>,
        total: usize,
        on_done: &Arc<dyn Fn(usize, usize, &ItemOutcome) + Send + Sync>,
        first_err: &Arc<std::sync::Mutex<Option<DownloadError>>>,
    ) {
        let mut copies: Vec<CopyJob> = Vec::new();
        let mut groups: Vec<(PathBuf, Vec<ItemSpec>)> = Vec::new();
        for item in items {
            let cache_hit = cache_path_for(&self.cache_dir, &item)
                .filter(|c| c.exists() && verify_cache_for(&item, c));
            match cache_hit {
                Some(c) => copies.push(CopyJob {
                    src: c,
                    from_cache: true,
                    item,
                }),
                None => match &item.fetch {
                    Fetch::ZipEntry { archive, .. } => match groups.iter_mut().find(|(a, _)| a == archive) {
                        Some(g) => g.1.push(item),
                        None => groups.push((archive.clone(), vec![item])),
                    },
                    Fetch::Local(p) => copies.push(CopyJob {
                        src: p.clone(),
                        from_cache: false,
                        item,
                    }),
                    Fetch::Url(_) => {} // 已按 URL 分区，理论不可达
                },
            }
        }

        let mut jobs: Vec<OfflineJob> = Vec::new();
        for ch in copies.chunks(COPY_CHUNK) {
            jobs.push(OfflineJob::Copy(ch.to_vec()));
        }
        for (archive, list) in groups {
            let n = offline::shard_count(list.len(), self.concurrency);
            for s in 0..n {
                let part: Vec<ItemSpec> = list
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| i % n == s)
                    .map(|(_, it)| it.clone())
                    .collect();
                jobs.push(OfflineJob::Extract(archive.clone(), part));
            }
        }

        let mut handles = Vec::with_capacity(jobs.len());
        // 任一分片失败即叫停其余分片（否则一个坏条目会引来几百次无谓落盘）
        let stop = Arc::new(AtomicBool::new(false));
        for job in jobs {
            let (cancel, done, on_done, first_err) =
                (cancel.clone(), done.clone(), on_done.clone(), first_err.clone());
            let stop2 = stop.clone();
            handles.push(tokio::task::spawn_blocking(move || {
                let r = match job {
                    OfflineJob::Copy(list) => {
                        offline::run_copies(list, &cancel, &stop2, &done, total, &*on_done)
                    }
                    OfflineJob::Extract(archive, list) => {
                        offline::run_extract(&archive, list, &cancel, &stop2, &done, total, &*on_done)
                    }
                };
                if let Err(e) = r {
                    stop2.store(true, Ordering::Relaxed);
                    let mut g = first_err.lock().unwrap();
                    if g.is_none() {
                        *g = Some(e);
                    }
                }
            }));
        }
        for h in handles {
            if let Err(e) = h.await {
                let mut g = first_err.lock().unwrap();
                if g.is_none() {
                    *g = Some(DownloadError::Failed {
                        file_name: "离线取件".to_string(),
                        attempts: 0,
                        cause: format!("取件线程异常：{e}"),
                    });
                }
            }
        }
    }

    /// 联网单项：缓存命中直接复制落位，否则流式取回。离线项由 harvest 通道处理，不进这里。
    async fn download_one(&self, item: &ItemSpec) -> Result<ItemOutcome, DownloadError> {
        let Fetch::Url(url) = &item.fetch else {
            unreachable!("离线项走 harvest_offline，download_all 不会把非 URL 项交给这里")
        };
        if let Some(parent) = item.dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // 缓存复用前提：声明了 sha1 就必须与内容一致（不一致视作缓存损坏，重新获取）
        let cache = self.cache_path_opt(item);
        let cached = cache
            .as_ref()
            .filter(|c| c.exists())
            .is_some_and(|c| self.verify_cache(item, c));
        if cached {
            let c = cache.unwrap();
            std::fs::copy(&c, &item.dest).map_err(|e| DownloadError::Failed {
                file_name: item.file_name.clone(),
                attempts: 0,
                cause: format!("{e}（dest={}）", item.dest.display()),
            })?;
            // 复用即续命：缓存清理按 mtime 判过期（见 core::cleanup）
            mark_used(&c);
            let bytes = std::fs::metadata(&c).map(|m| m.len()).unwrap_or(0);
            return Ok(ItemOutcome {
                file_name: item.file_name.clone(),
                source: FetchSource::Network,
                cached: true,
                bytes,
                retries: 0,
                dest: item.dest.clone(),
            });
        }

        let (bytes, retries) = self.stream_url(item, url, cache.as_deref()).await?;
        Ok(ItemOutcome {
            file_name: item.file_name.clone(),
            source: FetchSource::Network,
            cached: false,
            bytes,
            retries,
            dest: item.dest.clone(),
        })
    }

    /// 流式取回一个 URL：边收块边写临时文件、边增量算 sha1，校验通过才落到缓存与目标位。
    /// 旧写法 `resp.bytes()` 把整个 jar 囤在内存里、且收完之前一个字节进度都没有。
    ///
    /// 候选源按 `source::candidates` 顺序试（镜像在前、官方在后）：**非末位源只给一次机会**，
    /// 换源要快，不能把退避预算耗在一个明显不通的镜像上；最后一个源才吃满 `RETRIES`。
    /// 内容校验不过（`BadHost`）不换时间重试、直接换源——同一个镜像再下三遍还是错的。
    /// 返回的 retries 是「到成功为止一共失败了几次」，跨源累计，日志里就是用户看到的重试数。
    async fn stream_url(
        &self,
        item: &ItemSpec,
        url: &str,
        cache: Option<&Path>,
    ) -> Result<(u64, u32), DownloadError> {
        let candidates = source::candidates(url, self.source);
        let mut last_cause = String::from("unknown");
        let mut fails: u32 = 0;
        for (i, cand) in candidates.iter().enumerate() {
            let is_last = i + 1 == candidates.len();
            let budget = if is_last { RETRIES } else { 1 };
            for attempt in 1..=budget {
                match self.stream_attempt(item, cand, cache, attempt).await {
                    Ok(bytes) => return Ok((bytes, fails)),
                    Err(Attempt::Fatal(e)) => return Err(e),
                    Err(Attempt::BadHost(cause)) => {
                        last_cause = cause;
                        fails += 1;
                        break; // 内容不对：原地重试没意义，换下一个源
                    }
                    Err(Attempt::Retry(cause)) => {
                        last_cause = cause;
                        fails += 1;
                        if attempt < budget {
                            tokio::time::sleep(std::time::Duration::from_millis(
                                500 * attempt as u64,
                            ))
                            .await;
                        }
                    }
                }
            }
        }
        Err(DownloadError::Failed {
            file_name: item.file_name.clone(),
            attempts: fails,
            cause: last_cause,
        })
    }

    /// 单次尝试：成功返回落盘字节数。失败（含重试路径）一律清掉半截临时文件
    async fn stream_attempt(
        &self,
        item: &ItemSpec,
        url: &str,
        cache: Option<&Path>,
        attempt: u32,
    ) -> Result<u64, Attempt> {
        // 有缓存槽先落到缓存名（成功后复制到 dest）；无槽位则直接在 dest 旁边成形
        let staged: &Path = cache.unwrap_or(item.dest.as_path());
        if let Some(parent) = staged.parent() {
            std::fs::create_dir_all(parent).map_err(|e| io_err(item, attempt, &e, staged))?;
        }
        let temp = temp_path(staged, attempt);
        let written = match self.write_to_temp(item, url, &temp, attempt).await {
            Ok(w) => w,
            Err(e) => {
                // 文件句柄已在 write_to_temp 返回时关闭，这里才删得掉
                let _ = std::fs::remove_file(&temp);
                return Err(e);
            }
        };
        std::fs::rename(&temp, staged).map_err(|e| io_err(item, attempt, &e, staged))?;
        if staged != item.dest.as_path() {
            std::fs::copy(staged, &item.dest)
                .map_err(|e| io_err(item, attempt, &e, &item.dest))?;
        }
        Ok(written)
    }

    /// 收流写临时文件：边写边增量算 sha1，每个响应块向 transfer 出口报一次字节；
    /// 校验不通过算「这个源给错了东西」（BadHost）——换源再试，而不是原地重试
    async fn write_to_temp(
        &self,
        item: &ItemSpec,
        url: &str,
        temp: &Path,
        attempt: u32,
    ) -> Result<u64, Attempt> {
        use std::io::Write;
        use sha1::{Digest, Sha1};

        let resp = self.client.get(url).send().await.map_err(|e| {
            let status = e.status().map(|s| s.as_u16()).unwrap_or(0);
            Attempt::Retry(format!("{url} 请求失败（HTTP {status}）：{e}"))
        })?;
        let status = resp.status();
        if !status.is_success() {
            return Err(Attempt::Retry(format!("{url} → HTTP {}", status.as_u16())));
        }
        let total = resp.content_length().unwrap_or(0);

        let mut file =
            std::fs::File::create(temp).map_err(|e| io_err(item, attempt, &e, temp))?;
        let mut hasher = Sha1::new();
        let mut written: u64 = 0;
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| {
                let of = if total > 0 { total.to_string() } else { "?".into() };
                Attempt::Retry(format!("{url} 传输中断（已收 {written}/{of} 字节）：{e}"))
            })?;
            file.write_all(&chunk)
                .map_err(|e| io_err(item, attempt, &e, temp))?;
            hasher.update(&chunk);
            written += chunk.len() as u64;
            if let Some(t) = &self.transfer {
                t(&TransferProgress {
                    file_name: item.file_name.clone(),
                    key: item.dest.clone(),
                    done: written,
                    total,
                    attempt,
                });
            }
        }
        file.flush().map_err(|e| io_err(item, attempt, &e, temp))?;

        if let Some(expect) = &item.sha1 {
            let got: String = hasher
                .finalize()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            if !got.eq_ignore_ascii_case(expect) {
                return Err(Attempt::BadHost(format!(
                    "{url} sha1 校验不一致（期望 {expect} · 实际 {got} · 已收 {written} 字节）"
                )));
            }
        }
        Ok(written)
    }

    /// 缓存文件与声明 sha1 是否一致（未声明视为可用；读取失败按不一致处理，触发重取）
    fn verify_cache(&self, item: &ItemSpec, cache: &Path) -> bool {
        verify_cache_for(item, cache)
    }

    /// 该条目是否已在下载缓存（仅存在性判断，不重算哈希——预估宁可少扣不误报）
    pub fn is_cached(&self, item: &ItemSpec) -> bool {
        self.cache_path_opt(item).is_some_and(|c| c.exists())
    }

    /// HEAD 取 Content-Length（下载量预估的兜底大小来源）；进程级缓存按**官方 URL** 记账，
    /// 与下载源无关。预估随方案编辑高频触发，同一坐标的大小不会变。
    /// 某个源 HEAD 不通（镜像未同步、403）就换下一个候选，全失败才报无尺寸。
    pub async fn head_size(&self, url: &str) -> Option<u64> {
        use std::collections::HashMap;
        use std::sync::{Mutex, OnceLock};
        static SIZES: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
        let cache = SIZES.get_or_init(Default::default);
        let hit = cache.lock().unwrap().get(url).copied();
        if let Some(s) = hit {
            return Some(s);
        }
        for cand in source::candidates(url, self.source) {
            let len = self
                .client
                .head(&cand)
                .send()
                .await
                .ok()
                .and_then(|r| r.content_length());
            if let Some(len) = len.filter(|l| *l > 0) {
                cache.lock().unwrap().insert(url.to_string(), len);
                return Some(len);
            }
        }
        None
    }

    /// Maven 系 URL 带伴生 `<jar>.sha1` 文本：为无校验值的条目 best-effort 补上
    /// （Fabric meta 的 server/jar 端点无伴生文件，取不到则保持不校验）。
    /// 结果（含未命中）进进程级缓存：预估命令随方案编辑高频调用，同坐标伴生值不变。
    pub async fn attach_side_sha1(&self, spec: &mut ItemSpec) {
        use std::collections::HashMap;
        use std::sync::{Mutex, OnceLock};
        static SIDES: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();
        if spec.sha1.is_some() {
            return;
        }
        let Fetch::Url(url) = &spec.fetch else { return };
        let cache = SIDES.get_or_init(Default::default);
        if let Some(hit) = cache.lock().unwrap().get(url.as_str()) {
            spec.sha1 = hit.clone();
            return;
        }
        let found = match self.fetch_bytes(&format!("{url}.sha1")).await {
            Ok(bytes) => {
                let text = String::from_utf8_lossy(&bytes).trim().to_ascii_lowercase();
                if text.len() == 40 && text.chars().all(|c| c.is_ascii_hexdigit()) {
                    Some(text)
                } else {
                    None
                }
            }
            Err(_) => None,
        };
        cache.lock().unwrap().insert(url.clone(), found.clone());
        spec.sha1 = found;
    }

    /// 单次 GET，**不做下载源重写**：走哪条 URL 由调用方决定。
    /// `attach_side_sha1` 因此天然只信官方站的伴生校验值，不让镜像自证清白。
    async fn fetch_bytes(&self, url: &str) -> Result<Vec<u8>, DownloadError> {
        let resp = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| DownloadError::Http {
                url: url.to_string(),
                status: e.status().map(|s| s.as_u16()).unwrap_or(0),
            })?;
        let status = resp.status();
        if !status.is_success() {
            return Err(DownloadError::Http {
                url: url.to_string(),
                status: status.as_u16(),
            });
        }
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| DownloadError::Http {
                url: url.to_string(),
                status: e.status().map(|s| s.as_u16()).unwrap_or(0),
            })?;
        Ok(bytes.to_vec())
    }

    /// 读一个 JSON 文档：按下载源的候选链试（镜像在前、官方在后）。
    /// 解析失败也换源——镜像未同步时常给 200 + 错误页，只有官方那次的结果才算数。
    pub(crate) async fn get_json(&self, url: &str) -> Result<Value, DownloadError> {
        let mut last_err = DownloadError::NotFound(url.to_string());
        for cand in source::candidates(url, self.source) {
            match self.fetch_bytes(&cand).await.and_then(|b| {
                serde_json::from_slice(&b).map_err(DownloadError::from)
            }) {
                Ok(v) => return Ok(v),
                Err(e) => last_err = e,
            }
        }
        Err(last_err)
    }

    /// 读一个 JSON 值（POST 版本：`get_json` 只能取）
    pub(crate) async fn post_json(&self, url: &str, body: &Value) -> Result<Value, DownloadError> {
        let resp = self
            .client
            .post(url)
            .json(body)
            .send()
            .await
            .map_err(|e| DownloadError::Http {
                url: url.to_string(),
                status: e.status().map(|s| s.as_u16()).unwrap_or(0),
            })?;
        let status = resp.status();
        if !status.is_success() {
            return Err(DownloadError::Http {
                url: url.to_string(),
                status: status.as_u16(),
            });
        }
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| DownloadError::Http {
                url: url.to_string(),
                status: e.status().map(|s| s.as_u16()).unwrap_or(0),
            })?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    /// CurseForge 专用 JSON GET：挂 `x-api-key` 头，**不走镜像候选链**（`source` 模块表里 CF 没有镜像）。
    /// 缺 Key 与 Key 被拒都归 `Refused`：这两句是要原样显示给用户看的，前者要给「去配置」出口、
    /// 后者要说清是 Key 的问题而不是网络的问题。错误文案里绝不出现 Key 本身
    pub(crate) async fn cf_get_json(&self, url: &str) -> Result<Value, DownloadError> {
        let Some(key) = self.cf_key.as_deref() else {
            return Err(DownloadError::Refused(
                "还没有配置 CurseForge API Key：在「设置 · 网络 · CurseForge API Key」填一把，\
                 或在 CurseForge 官方表单免费申请"
                    .into(),
            ));
        };
        let resp = self
            .client
            .get(url)
            .header("x-api-key", key)
            .send()
            .await
            .map_err(|e| DownloadError::Http {
                url: url.to_string(),
                status: e.status().map(|s| s.as_u16()).unwrap_or(0),
            })?;
        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN
        {
            return Err(DownloadError::Refused(format!(
                "CurseForge 拒绝了这次请求（HTTP {}）：API Key 无效、过期或没有该接口权限",
                status.as_u16()
            )));
        }
        if !status.is_success() {
            return Err(DownloadError::Http {
                url: url.to_string(),
                status: status.as_u16(),
            });
        }
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| DownloadError::Http {
                url: url.to_string(),
                status: e.status().map(|s| s.as_u16()).unwrap_or(0),
            })?;
        Ok(serde_json::from_slice(&bytes)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::downloader::util::sha1_hex;
    use std::fs::File;

    /// 造一个含指定条目的 zip，返回归档路径（放在独立临时目录里，便于同时放本地文件与输出）
    fn temp_zip(entries: &[(&str, &[u8])]) -> PathBuf {
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("sideshift-dl-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pack.zip");
        let mut w = zip::ZipWriter::new(File::create(&path).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        for (name, bytes) in entries {
            w.start_file(name.to_string(), opts).unwrap();
            w.write_all(bytes).unwrap();
        }
        let _ = w.finish().unwrap();
        path
    }

    fn zip_item(archive: &Path, entry: &str, dest: PathBuf, sha1: Option<String>) -> ItemSpec {
        ItemSpec {
            fetch: Fetch::ZipEntry {
                archive: archive.to_path_buf(),
                entry: entry.to_string(),
            },
            file_name: entry.rsplit('/').next().unwrap().to_string(),
            sha1,
            dest,
            size_bytes: 0,
        }
    }

    #[tokio::test]
    async fn offline_items_land_without_network() {
        let archive = temp_zip(&[("entries/a.txt", b"alpha".as_slice()), ("entries/b.txt", b"bravo".as_slice())]);
        let root = archive.parent().unwrap().to_path_buf();
        let out = root.join("out");
        let local = root.join("local.jar");
        std::fs::write(&local, b"local-bytes").unwrap();

        let dl = Downloader::new(out.join("cache"), 4);
        let items = vec![
            zip_item(&archive, "entries/a.txt", out.join("a.txt"), None),
            zip_item(
                &archive,
                "entries/b.txt",
                out.join("b.txt"),
                Some(sha1_hex(b"bravo")),
            ),
            ItemSpec {
                fetch: Fetch::Local(local.clone()),
                file_name: "local.jar".to_string(),
                sha1: None,
                dest: out.join("local.jar"),
                size_bytes: 0,
            },
        ];
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen2 = seen.clone();
        dl.download_all(items, Arc::new(AtomicBool::new(false)), move |_done, _tot, oc| {
            seen2.lock().unwrap().push((oc.source, oc.bytes, oc.cached));
        })
        .await
        .unwrap();

        let got = seen.lock().unwrap();
        assert_eq!(got.len(), 3, "每条都应回一次");
        assert!(got.iter().all(|(_, _, cached)| !cached), "离线项不该报缓存命中");
        assert_eq!(std::fs::read_to_string(out.join("a.txt")).unwrap(), "alpha");
        assert_eq!(std::fs::read_to_string(out.join("b.txt")).unwrap(), "bravo");
        assert_eq!(std::fs::read_to_string(out.join("local.jar")).unwrap(), "local-bytes");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn sha1_mismatch_fails_and_leaves_no_partial_file() {
        let archive = temp_zip(&[("entries/a.txt", b"alpha".as_slice())]);
        let root = archive.parent().unwrap().to_path_buf();
        let out = root.join("out");
        let dest = out.join("a.txt");
        let dl = Downloader::new(out.join("cache"), 4);
        let items = vec![zip_item(&archive, "entries/a.txt", dest.clone(), Some(sha1_hex(b"nope")))];
        let r = dl
            .download_all(items, Arc::new(AtomicBool::new(false)), |_, _, _| {})
            .await;
        assert!(matches!(r, Err(DownloadError::Failed { attempts: 0, .. })));
        assert!(!dest.exists(), "校验失败的半成品必须删掉");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 规模回归：复刻真实卡死场景的量级——保留目录命中后一次转换要取几千个包内小文件。
    /// 只验两件事：**不挂起**（回调必须逐条收齐）与**内容正确**（带 sha1 的条目全部落位）。
    #[tokio::test]
    async fn bulk_offline_harvest_completes() {
        const N: usize = 3000;
        let entries: Vec<(String, Vec<u8>)> = (0..N)
            .map(|i| {
                let name = format!("overrides/config/mod-{i}/file-{i}.cfg");
                (name, format!("content-{i}-{}", i * 37).into_bytes())
            })
            .collect();
        let refs: Vec<(&str, &[u8])> = entries
            .iter()
            .map(|(n, b)| (n.as_str(), b.as_slice()))
            .collect();
        let archive = temp_zip(&refs);
        let root = archive.parent().unwrap().to_path_buf();
        let out = root.join("out");

        let items: Vec<ItemSpec> = entries
            .iter()
            .map(|(name, bytes)| {
                zip_item(
                    &archive,
                    name,
                    out.join(name.rsplit('/').next().unwrap()),
                    Some(sha1_hex(bytes)),
                )
            })
            .collect();

        let dl = Downloader::new(out.join("cache"), 6);
        // 分片并行 → 回调到达顺序不保证与 done 同序，只能验「每个 done 恰好出现一次」
        let seen = Arc::new(std::sync::Mutex::new(std::collections::BTreeSet::new()));
        let seen2 = seen.clone();
        let out2 = out.clone();
        dl.download_all(items, Arc::new(AtomicBool::new(false)), move |done, total, oc| {
            assert!(seen2.lock().unwrap().insert(done), "done={done} 重复回调");
            assert_eq!(total, N);
            assert!(!oc.cached && matches!(oc.source, FetchSource::Pack));
            // 分组日志靠 dest 定位，批量解包路径也必须带上真实落位
            assert_eq!(oc.dest.file_name().and_then(|s| s.to_str()), Some(oc.file_name.as_str()));
            assert!(oc.dest.starts_with(&out2), "dest 应在取件目录内：{}", oc.dest.display());
        })
        .await
        .unwrap();

        let done_ids = seen.lock().unwrap();
        assert_eq!(done_ids.len(), N, "每条都必须回调一次（卡死即少条）");
        assert_eq!(done_ids.first().copied(), Some(1));
        assert_eq!(done_ids.last().copied(), Some(N));
        assert_eq!(std::fs::read_dir(&out).unwrap().count(), N);
        assert_eq!(
            std::fs::read_to_string(out.join("file-2999.cfg")).unwrap(),
            format!("content-2999-{}", 2999 * 37)
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 流式下载：边收边写临时文件 → 校验通过才 rename 到缓存并复制到 dest；
    /// 中途写入的 dest 只能是完整内容，且 transfer 回调必须带单调增长的字节数
    #[tokio::test]
    async fn streaming_download_reports_progress_and_writes_complete_file() {
        use std::io::{Read, Write};

        let payload: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
        let want_sha1 = sha1_hex(&payload);

        // 一次性本地 HTTP 服务：固定 Content-Length，分块写出以触发多次进度回调
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let body = payload.clone();
        let srv = tokio::task::spawn_blocking(move || {
            let (mut sock, _) = listener.accept().unwrap();
            let mut req = [0u8; 4096];
            let _ = sock.read(&mut req);
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            sock.write_all(head.as_bytes()).unwrap();
            for chunk in body.chunks(50_000) {
                sock.write_all(chunk).unwrap();
            }
            sock.flush().unwrap();
        });

        let dir = std::env::temp_dir().join(format!("sideshift-dl-{}", uuid::Uuid::new_v4()));
        let out = dir.join("out");
        let dest = out.join("blob.bin");
        let progress: Arc<std::sync::Mutex<Vec<(u64, u64, u32)>>> = Default::default();
        let sink = {
            let progress = progress.clone();
            move |p: &TransferProgress| {
                progress.lock().unwrap().push((p.done, p.total, p.attempt))
            }
        };
        let dl = Downloader::new(out.join("cache"), 2).with_transfer(Arc::new(sink));
        let items = vec![ItemSpec {
            fetch: Fetch::Url(format!("http://{addr}/blob.bin")),
            file_name: "blob.bin".to_string(),
            sha1: Some(want_sha1.clone()),
            dest: dest.clone(),
            size_bytes: 0,
        }];
        dl.download_all(items, Arc::new(AtomicBool::new(false)), |_, _, _| {})
            .await
            .unwrap();
        srv.await.unwrap();

        assert_eq!(std::fs::read(&dest).unwrap(), payload, "dest 必须是完整内容");
        // 进度单调不减，且至少覆盖总量
        let p = progress.lock().unwrap();
        assert!(p.len() >= 2, "分块写入应有多次进度回调，实得 {}", p.len());
        assert!(p.windows(2).all(|w| w[0].0 <= w[1].0), "done 必须单调不减");
        assert_eq!(p.last().unwrap().0, payload.len() as u64);
        assert_eq!(p.last().unwrap().1, payload.len() as u64, "应取到 Content-Length");
        // 缓存位已成形，且没有半截临时文件残留
        let cache = out.join("cache").join("files").join(&want_sha1).join("blob.bin");
        assert!(cache.exists(), "成功后应落到缓存：{}", cache.display());
        let mut leftovers = 0;
        let mut stack = vec![out.join("cache")];
        while let Some(d) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&d) else { continue };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.file_name().unwrap().to_string_lossy().contains(".part") {
                    leftovers += 1;
                }
            }
        }
        assert_eq!(leftovers, 0, "临时文件必须清干净");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
