//! 下载器与外部 API：并发限流 + sha1 缓存 + 3 次重试；Modrinth 搜索/版本解析；
//! MC 版本表（piston-meta）、Fabric/Forge/NeoForge 加载器版本表。

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use futures::stream::{self, StreamExt};
use reqwest::Client;
use serde_json::Value;
use thiserror::Error;

use crate::models::{LoaderKind, ModSearchPage, ModSearchQuery, ModSearchResult, ModSource, ModVersionEntry, VersionOption};

pub const MODRINTH_API: &str = "https://api.modrinth.com/v2";
const PISTON_MANIFEST: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const FABRIC_META: &str = "https://meta.fabricmc.net/v2";
const FORGE_PROMOTIONS: &str = "https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json";
/// 每个 MC 版本的全部 Forge 构建：{ "1.20.1": ["1.20.1-47.4.10", …], … }
const FORGE_MAVEN_META: &str = "https://files.minecraftforge.net/net/minecraftforge/forge/maven-metadata.json";
const NEOFORGE_VERSIONS: &str =
    "https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/neoforge";
const USER_AGENT: &str = "SideShift/0.1 (desktop pack converter)";
const RETRIES: u32 = 3;

#[derive(Error, Debug)]
pub enum DownloadError {
    #[error("网络请求失败：{url}（HTTP {status}）")]
    Http { url: String, status: u16 },
    #[error("文件读写失败：{0}")]
    Io(#[from] std::io::Error),
    #[error("数据解析失败：{0}")]
    Api(#[from] serde_json::Error),
    #[error("下载失败（已重试 {attempts} 次）：{file_name} — {cause}")]
    Failed {
        file_name: String,
        attempts: u32,
        cause: String,
    },
    #[error("未找到可用版本：{0}")]
    NotFound(String),
}

/// 文件来源：远程 URL、本地 zip 包内条目（裸 zip 整合包免网络直提）、或本地单文件
#[derive(Debug, Clone)]
pub enum Fetch {
    Url(String),
    ZipEntry { archive: PathBuf, entry: String },
    Local(PathBuf),
}

/// 一个待获取文件
#[derive(Debug, Clone)]
pub struct ItemSpec {
    pub fetch: Fetch,
    pub file_name: String,
    pub sha1: Option<String>,
    /// 最终落盘绝对路径（含文件名）
    pub dest: PathBuf,
    /// 源文件大小（字节）；0 = 未知。构建不消费此字段，仅供下载量预估聚合
    pub size_bytes: u64,
}

/// 取件来源（界面据此区分「下载」与「取件」）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchSource {
    /// 需联网
    Network,
    /// 整合包内直取
    Pack,
    /// 本地 jar 复制
    Local,
}

/// 单条目取件结果：联网项逐条报，离线项由流水线按落位目录聚合后再报
#[derive(Debug, Clone)]
pub struct ItemOutcome {
    pub file_name: String,
    pub source: FetchSource,
    pub cached: bool,
    pub bytes: u64,
    /// 联网重试次数（0 = 一次成功）
    pub retries: u32,
    /// 实际落位绝对路径：流水线据此归入「模组 / 各保留目录」分组
    pub dest: PathBuf,
}

/// 逐条完成回调：Arc 持有与借用两种传法共用同一签名
type OnDone = dyn Fn(usize, usize, &ItemOutcome) + Send + Sync;

/// 联网传输中的字节进度（每个响应块一次，调用方自行节流；离线搬运不发）
#[derive(Debug, Clone)]
pub struct TransferProgress {
    pub file_name: String,
    /// 本条目的唯一键（dest 绝对路径）：并发下载同名条目也不串账
    pub key: PathBuf,
    /// 本文件已收字节
    pub done: u64,
    /// 本文件总字节，0 = 响应无 Content-Length
    pub total: u64,
    /// 第几次尝试（1 起）
    pub attempt: u32,
}

type OnTransfer = dyn Fn(&TransferProgress) + Send + Sync;

/// 单次下载尝试的失败分类
enum Attempt {
    /// 换个时间再试可能成功（连接重置、429/5xx）：退避后重试
    Retry(String),
    /// 重试也不会变好（sha1 不符、磁盘写不进）：立即结束本条目
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
    target.with_file_name(format!("{stem}.part{attempt}"))
}

impl ItemSpec {
    fn source_key(&self) -> String {
        match &self.fetch {
            Fetch::Url(u) => u.clone(),
            Fetch::ZipEntry { archive, entry } => format!("{}#{entry}", archive.display()),
            Fetch::Local(p) => p.to_string_lossy().to_string(),
        }
    }
}

pub struct Downloader {
    pub client: Client,
    cache_dir: PathBuf,
    concurrency: usize,
    transfer: Option<Arc<OnTransfer>>,
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
        }
    }

    /// 挂上字节进度出口：流水线据此渲染「联网下载中」实时条
    pub fn with_transfer(mut self, f: Arc<OnTransfer>) -> Self {
        self.transfer = Some(f);
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
                        Ok(outcome) => report(done.as_ref(), total, &*on_done, outcome),
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
            let n = shard_count(list.len(), self.concurrency);
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
                        run_copies(list, &cancel, &stop2, &done, total, &*on_done)
                    }
                    OfflineJob::Extract(archive, list) => {
                        run_extract(&archive, list, &cancel, &stop2, &done, total, &*on_done)
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
    /// 传输类失败退避重试至多 RETRIES 次；校验/磁盘类失败就地结束。
    async fn stream_url(
        &self,
        item: &ItemSpec,
        url: &str,
        cache: Option<&Path>,
    ) -> Result<(u64, u32), DownloadError> {
        let mut last_cause = String::from("unknown");
        for attempt in 1..=RETRIES {
            match self.stream_attempt(item, url, cache, attempt).await {
                Ok(bytes) => return Ok((bytes, attempt - 1)),
                Err(Attempt::Fatal(e)) => return Err(e),
                Err(Attempt::Retry(cause)) => {
                    last_cause = cause;
                    if attempt < RETRIES {
                        tokio::time::sleep(std::time::Duration::from_millis(
                            500 * attempt as u64,
                        ))
                        .await;
                    }
                }
            }
        }
        Err(DownloadError::Failed {
            file_name: item.file_name.clone(),
            attempts: RETRIES,
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
    /// 校验不通过算确定性失败（Fatal），不占用重试次数
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
                return Err(Attempt::Fatal(DownloadError::Failed {
                    file_name: item.file_name.clone(),
                    attempts: attempt.saturating_sub(1),
                    cause: format!(
                        "{url} sha1 校验不一致（期望 {expect} · 实际 {got} · 已收 {written} 字节）"
                    ),
                }));
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

    /// HEAD 取 Content-Length（下载量预估的兜底大小来源）；进程级缓存，
    /// 预估随方案编辑高频触发，同一坐标的大小不会变
    pub async fn head_size(&self, url: &str) -> Option<u64> {
        use std::collections::HashMap;
        use std::sync::{Mutex, OnceLock};
        static SIZES: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
        let cache = SIZES.get_or_init(Default::default);
        if let Some(s) = cache.lock().unwrap().get(url) {
            return Some(*s);
        }
        let resp = self.client.head(url).send().await.ok()?;
        let len = resp.content_length()?;
        if len > 0 {
            cache.lock().unwrap().insert(url.to_string(), len);
        }
        Some(len)
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

    async fn get_json(&self, url: &str) -> Result<Value, DownloadError> {
        let bytes = self.fetch_bytes(url).await?;
        let v: Value = serde_json::from_slice(&bytes)?;
        Ok(v)
    }

    /* ---------------- Modrinth ---------------- */

    pub async fn search_mods(&self, q: &ModSearchQuery) -> Result<ModSearchPage, DownloadError> {
        if q.source == ModSource::Curseforge {
            return Err(DownloadError::NotFound(
                "CurseForge 搜索需要 API Key，一期请使用 Modrinth".into(),
            ));
        }
        let page_size = 20u32;
        let offset = (q.page.saturating_sub(1)) * page_size;
        let mut facets: Vec<Vec<String>> = Vec::new();
        if !q.mc_version.is_empty() {
            facets.push(vec![format!("versions:{}", q.mc_version)]);
        }
        if let Some(l) = q.loader {
            facets.push(vec![format!("categories:{}", loader_cat(l))]);
        }
        if let Some(cat) = &q.category {
            if !cat.is_empty() && cat != "all" {
                facets.push(vec![format!("categories:{}", cat.to_lowercase())]);
            }
        }
        let index = if q.text.trim().is_empty() {
            "downloads"
        } else {
            "relevance"
        };
        let facets_json = serde_json::to_string(&facets).unwrap_or_else(|_| "[]".into());
        let url = format!(
            "{MODRINTH_API}/search?query={}&limit={page_size}&offset={offset}&index={}&facets={}",
            urlencoding(q.text.trim()),
            index,
            urlencoding(&facets_json)
        );
        let v = self.get_json(&url).await?;
        let total = v["total_hits"].as_u64().unwrap_or(0);
        let mut results = Vec::new();
        if let Some(hits) = v["hits"].as_array() {
            for h in hits {
                results.push(ModSearchResult {
                    id: h["slug"].as_str().unwrap_or_default().to_string(),
                    name: h["title"].as_str().unwrap_or_default().to_string(),
                    description: h["description"].as_str().unwrap_or_default().to_string(),
                    author: h["author"].as_str().unwrap_or_default().to_string(),
                    downloads: h["downloads"].as_u64().unwrap_or(0),
                    icon_url: h["icon_url"].as_str().map(String::from),
                    source: ModSource::Modrinth,
                    compatible: true,
                    already_added: false, // 由 commands 层按当前方案回填
                });
            }
        }
        Ok(ModSearchPage {
            source: ModSource::Modrinth,
            total,
            results,
            page: q.page,
            page_size,
        })
    }

    /// 某模组的可用构建（按请求 MC 版本兼容性排序）
    pub async fn list_mod_versions(
        &self,
        mod_id: &str,
        mc_version: &str,
    ) -> Result<Vec<ModVersionEntry>, DownloadError> {
        let url = format!("{MODRINTH_API}/project/{mod_id}/version");
        let v = self.get_json(&url).await?;
        let mut entries = Vec::new();
        if let Some(arr) = v.as_array() {
            for e in arr {
                let game_versions: Vec<String> = e["game_versions"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|g| g.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                let loaders: Vec<String> = e["loaders"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|g| g.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                let Some(file) = e["files"].as_array().and_then(|f| {
                    f.iter()
                        .find(|x| x["primary"].as_bool().unwrap_or(false))
                        .or(f.first())
                }) else {
                    continue;
                };
                let mc = if game_versions.iter().any(|g| g == mc_version) {
                    mc_version.to_string()
                } else {
                    game_versions.first().cloned().unwrap_or_default()
                };
                let loader = loaders
                    .iter()
                    .find(|l| **l == "fabric" || **l == "forge" || **l == "neoforge")
                    .and_then(|l| match l.as_str() {
                        "fabric" => Some(LoaderKind::Fabric),
                        "forge" => Some(LoaderKind::Forge),
                        "neoforge" => Some(LoaderKind::NeoForge),
                        _ => None,
                    })
                    .unwrap_or(LoaderKind::Fabric);
                entries.push(ModVersionEntry {
                    id: e["id"].as_str().unwrap_or_default().to_string(),
                    version_number: e["version_number"].as_str().unwrap_or_default().to_string(),
                    mc_version: mc,
                    loader,
                    date: e["date_published"]
                        .as_str()
                        .map(|s| s.chars().take(10).collect())
                        .unwrap_or_default(),
                    size_bytes: file["size"].as_u64().unwrap_or(0),
                    recommended: false,
                    url: file["url"].as_str().unwrap_or_default().to_string(),
                    sha1: file["hashes"]["sha1"].as_str().map(String::from),
                    file_name: file["filename"].as_str().unwrap_or("mod.jar").to_string(),
                });
            }
        }
        // 兼容请求 MC 版本的构建排前，组内按发布日期倒序；首个标推荐
        entries.sort_by(|a, b| {
            (a.mc_version != mc_version).cmp(&(b.mc_version != mc_version)).then(
                b.date.cmp(&a.date),
            )
        });
        if let Some(first) = entries.first_mut() {
            first.recommended = true;
        }
        Ok(entries)
    }

    /// Modrinth 官方类别标签（GET /tag/category，取模组向条目去重排序），供「类别」下拉
    pub async fn list_mod_categories(&self) -> Result<Vec<String>, DownloadError> {
        let v = self.get_json(&format!("{MODRINTH_API}/tag/category")).await?;
        let mut out: Vec<String> = Vec::new();
        if let Some(arr) = v.as_array() {
            for e in arr {
                let pt = e["project_type"].as_str().unwrap_or("");
                if pt != "mod" && pt != "all" {
                    continue;
                }
                if let Some(n) = e["name"].as_str() {
                    if !out.iter().any(|x| x == n) {
                        out.push(n.to_string());
                    }
                }
            }
        }
        out.sort();
        Ok(out)
    }

    /// 解析某模组在 (mc, loader) 下最新 release 构建的下载地址
    pub async fn resolve_mod_file(
        &self,
        mod_id: &str,
        mc_version: &str,
        loader: LoaderKind,
    ) -> Result<ItemSpec, DownloadError> {
        let url = format!(
            "{MODRINTH_API}/project/{mod_id}/version?game_versions={}&loaders={}",
            urlencoding(&format!("[\"{mc_version}\"]")),
            urlencoding(&format!("[\"{}\"]", loader_cat(loader)))
        );
        let v = self.get_json(&url).await?;
        let arr = v.as_array().ok_or_else(|| DownloadError::NotFound(mod_id.into()))?;
        for e in arr {
            if e["version_type"].as_str() != Some("release") {
                continue;
            }
            if let Some(file) = e["files"].as_array().and_then(|f| {
                f.iter()
                    .find(|x| x["primary"].as_bool().unwrap_or(false))
                    .or(f.first())
            }) {
                let Some(u) = file["url"].as_str() else { continue };
                return Ok(ItemSpec {
                    fetch: Fetch::Url(u.to_string()),
                    file_name: file["filename"].as_str().unwrap_or("mod.jar").to_string(),
                    sha1: file["hashes"]["sha1"].as_str().map(String::from),
                    dest: PathBuf::new(), // 由调用方补目标路径
                    size_bytes: file["size"].as_u64().unwrap_or(0),
                });
            }
        }
        Err(DownloadError::NotFound(format!(
            "{mod_id}（{mc_version} / {}）",
            loader.as_label()
        )))
    }

    /* ---------------- 版本表 ---------------- */

    /// 全量版本清单（接口本就一次返回，不截断，前端搜索即全量过滤）。
    /// 收 正式版 + Beta + Alpha 以兼容老整合包；快照（400+ 条）不进选择器。
    pub async fn list_mc_versions(&self) -> Result<Vec<VersionOption>, DownloadError> {
        let v = self.get_json(PISTON_MANIFEST).await?;
        let mut out = Vec::new();
        if let Some(arr) = v["versions"].as_array() {
            for e in arr {
                let group = match e["type"].as_str() {
                    Some("release") => "正式版",
                    Some("old_beta") => "Beta",
                    Some("old_alpha") => "Alpha",
                    _ => continue,
                };
                let id = e["id"].as_str().unwrap_or_default().to_string();
                out.push(VersionOption {
                    recommended: None,
                    group: Some(group.into()),
                    label: id.clone(),
                    value: id,
                });
            }
        }
        if let Some(first) = out.first_mut() {
            first.recommended = Some(true);
        }
        Ok(out)
    }

    pub async fn list_loader_versions(
        &self,
        mc_version: &str,
        loader: LoaderKind,
    ) -> Result<Vec<VersionOption>, DownloadError> {
        match loader {
            LoaderKind::Fabric => {
                let url = format!("{FABRIC_META}/versions/loader/{mc_version}");
                let v = self.get_json(&url).await?;
                let mut out = Vec::new();
                if let Some(arr) = v.as_array() {
                    for e in arr {
                        if e["loader"]["stable"].as_bool() != Some(true) {
                            continue;
                        }
                        let ver = e["loader"]["version"].as_str().unwrap_or_default().to_string();
                        out.push(VersionOption {
                            value: ver.clone(),
                            label: ver,
                            recommended: None,
                            group: None,
                        });
                    }
                }
                if let Some(first) = out.first_mut() {
                    first.recommended = Some(true);
                }
                Ok(out)
            }
            LoaderKind::Forge => {
                // 双官方接口：promotions_slim 只给每个 MC 版本的 recommended/latest 两个
                // 指针（键 "{mc}-{kind}"），maven-metadata 才含该版本全部构建。
                // 任一接口挂掉降级用另一个，双双失败才报错。
                let promos_res = self.get_json(FORGE_PROMOTIONS).await;
                let meta_res = self.get_json(FORGE_MAVEN_META).await;
                let (promos, meta) = match (promos_res, meta_res) {
                    (Err(e), Err(_)) => return Err(e),
                    (p, m) => (p.ok(), m.ok()),
                };
                let prefix = format!("{mc_version}-");
                let strip = |s: &str| s.strip_prefix(&prefix).unwrap_or(s).to_string();
                let promo = |kind: &str| {
                    promos
                        .as_ref()?
                        .get("promos")?
                        .get(format!("{mc_version}-{kind}"))?
                        .as_str()
                        .map(|s| strip(s))
                };
                let mut all: Vec<String> = meta
                    .as_ref()
                    .and_then(|m| m.get(mc_version))
                    .and_then(|a| a.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|x| x.as_str().map(|s| strip(s)))
                            .collect()
                    })
                    .unwrap_or_default();
                // 构建号倒序（version_cmp 按点分数字段比较）
                all.sort_by(|a, b| version_cmp(b, a));

                let mut out: Vec<VersionOption> = Vec::new();
                let mut seen = std::collections::HashSet::new();
                let mut add = |ver: String, group: &str, recommended: Option<bool>| {
                    if seen.insert(ver.clone()) {
                        out.push(VersionOption {
                            value: ver.clone(),
                            label: ver,
                            recommended,
                            group: Some(group.into()),
                        });
                    }
                };
                if let Some(r) = promo("recommended") {
                    add(r, "推荐", Some(true));
                }
                if let Some(l) = promo("latest") {
                    add(l, "最新", Some(false));
                }
                for ver in all {
                    add(ver, "全部构建", None);
                }
                Ok(out)
            }
            LoaderKind::NeoForge => {
                let v = self.get_json(NEOFORGE_VERSIONS).await?;
                // NeoForge 主版本映射 MC：20.x → 1.20.x, 21.x → 1.21.x …
                let mc_minor: Option<u32> = mc_version
                    .split('.')
                    .nth(1)
                    .and_then(|s| s.parse().ok());
                let mut out = Vec::new();
                if let (Some(minor), Some(arr)) = (mc_minor, v["versions"].as_array()) {
                    let mut vers: Vec<String> = arr
                        .iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .filter(|s| s.split('.').next().and_then(|p| p.parse::<u32>().ok()) == Some(minor + 19))
                        .collect();
                    vers.sort_by(|a, b| version_cmp(b, a));
                    for ver in vers.into_iter().take(15) {
                        out.push(VersionOption {
                            value: ver.clone(),
                            label: ver,
                            recommended: None,
                            group: None,
                        });
                    }
                    if let Some(first) = out.first_mut() {
                        first.recommended = Some(true);
                    }
                }
                Ok(out)
            }
        }
    }

    /// Fabric 一体化服务端 jar（内含 vanilla + loader）
    pub async fn fabric_server_jar(
        &self,
        game: &str,
        loader: &str,
    ) -> Result<ItemSpec, DownloadError> {
        let installer = self
            .get_json(&format!("{FABRIC_META}/versions/installer"))
            .await
            .ok()
            .and_then(|v| {
                v.as_array().and_then(|a| {
                    a.iter()
                        .find(|e| e["stable"].as_bool().unwrap_or(false))
                        .or(a.first())
                        .map(|e| e["version"].as_str().unwrap_or("1.0.1").to_string())
                })
            })
            .unwrap_or_else(|| "1.0.1".into());
        Ok(ItemSpec {
            fetch: Fetch::Url(format!(
                "{FABRIC_META}/versions/loader/{game}/{loader}/{installer}/server/jar"
            )),
            file_name: format!("fabric-server-mc.{game}.{loader}.{installer}.jar"),
            sha1: None,
            dest: PathBuf::new(),
            size_bytes: 0,
        })
    }

    /// Forge 官方 installer jar（一期：附带 installer + 首次安装脚本）
    pub fn forge_installer(&self, mc_version: &str, forge_version: &str) -> ItemSpec {
        let full = format!("{mc_version}-{forge_version}");
        ItemSpec {
            fetch: Fetch::Url(format!(
                "https://maven.minecraftforge.net/net/minecraftforge/forge/{full}/forge-{full}-installer.jar"
            )),
            file_name: format!("forge-{full}-installer.jar"),
            sha1: None,
            dest: PathBuf::new(),
            size_bytes: 0,
        }
    }

    /// NeoForge installer jar
    pub fn neoforge_installer(&self, neoforge_version: &str) -> ItemSpec {
        ItemSpec {
            fetch: Fetch::Url(format!(
                "https://maven.neoforged.net/releases/net/neoforged/neoforge/{neoforge_version}/neoforge-{neoforge_version}-installer.jar"
            )),
            file_name: format!("neoforge-{neoforge_version}-installer.jar"),
            sha1: None,
            dest: PathBuf::new(),
            size_bytes: 0,
        }
    }
}

fn loader_cat(loader: LoaderKind) -> &'static str {
    match loader {
        LoaderKind::Fabric => "fabric",
        LoaderKind::Forge => "forge",
        LoaderKind::NeoForge => "neoforge",
    }
}

/// 极简 URL 编码（查询参数场景）
fn urlencoding(s: &str) -> String {
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
fn sha1_hex(bytes: &[u8]) -> String {
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
fn version_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let pa: Vec<u64> = a.split('.').filter_map(|s| s.parse().ok()).collect();
    let pb: Vec<u64> = b.split('.').filter_map(|s| s.parse().ok()).collect();
    pa.cmp(&pb)
}

/* ---------------- 离线批量取件（download_all 的 harvest 通道） ---------------- */

/// 一次纯复制型任务：缓存命中或本地 jar → 目标路径
#[derive(Clone)]
struct CopyJob {
    src: PathBuf,
    /// 区分日志动词与「是否算联网件」口径
    from_cache: bool,
    item: ItemSpec,
}

/// 离线分片：一批复制，或某个归档的一个条目分片（分片内共用一个包句柄）
enum OfflineJob {
    Copy(Vec<CopyJob>),
    Extract(PathBuf, Vec<ItemSpec>),
}

/// 复制分片大小：小文件落盘在 Windows 上按次收费（含杀软拦截），分片才能并行
const COPY_CHUNK: usize = 64;

fn cache_path_for(cache_dir: &Path, item: &ItemSpec) -> Option<PathBuf> {
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
fn verify_cache_for(item: &ItemSpec, cache: &Path) -> bool {
    let Some(expect) = &item.sha1 else {
        return true;
    };
    let bytes = std::fs::read(cache).unwrap_or_default();
    sha1_hex(&bytes).eq_ignore_ascii_case(expect)
}

fn report(done: &AtomicUsize, total: usize, on_done: &OnDone, outcome: ItemOutcome) {
    let n = done.fetch_add(1, Ordering::Relaxed) + 1;
    on_done(n, total, &outcome);
}

/// 用户取消或任一分片已失败：本分片就地收摊
fn aborted(cancel: &AtomicBool, stop: &AtomicBool) -> bool {
    cancel.load(Ordering::Relaxed) || stop.load(Ordering::Relaxed)
}

/// 分片数：小批量单线程更省（每片都要解析一次中央目录），大批量才值得并行
fn shard_count(n: usize, concurrency: usize) -> usize {
    if n <= 16 {
        return 1;
    }
    concurrency.clamp(1, 8).min(n.div_ceil(32)).max(1)
}

fn run_copies(
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
fn run_extract(
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

#[cfg(test)]
mod tests {
    use super::*;

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
