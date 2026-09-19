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
        }
    }

    fn cache_path(&self, item: &ItemSpec) -> PathBuf {
        let key = item
            .sha1
            .clone()
            .unwrap_or_else(|| url_hash(&item.source_key()));
        self.cache_dir.join("files").join(key).join(&item.file_name)
    }

    /// 并发下载全部条目（各自带 dest 绝对路径）；cancel 置位后未开始的文件直接跳过；
    /// 每完成一个文件回调 (done, total)
    pub async fn download_all(
        &self,
        items: Vec<ItemSpec>,
        cancel: Arc<AtomicBool>,
        on_done: impl Fn(usize, usize) + Send + Sync,
    ) -> Result<(), DownloadError> {
        let total = items.len();
        let done = Arc::new(AtomicUsize::new(0));
        let on_done = Arc::new(on_done);

        let first_err: Arc<std::sync::Mutex<Option<DownloadError>>> =
            Arc::new(std::sync::Mutex::new(None));

        stream::iter(items)
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
                        Ok(()) => {
                            let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                            on_done(n as usize, total);
                        }
                        Err(e) => {
                            *first_err.lock().unwrap() = Some(e);
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

    async fn download_one(&self, item: &ItemSpec) -> Result<(), DownloadError> {
        let cache = self.cache_path(item);
        // 缓存复用前提：声明了 sha1 就必须与内容一致（不一致视作缓存损坏，重新获取）
        let cache_hit = cache.exists() && self.verify_cache(item, &cache);
        if !cache_hit {
            let bytes = match &item.fetch {
                Fetch::ZipEntry { archive, entry } => read_zip_entry(archive, entry)?,
                Fetch::Local(p) => std::fs::read(p).map_err(|e| DownloadError::Failed {
                    file_name: item.file_name.clone(),
                    attempts: 0,
                    cause: format!("本地文件读取失败：{e}（{}）", p.display()),
                })?,
                Fetch::Url(url) => {
                    let url = url.clone();
                    let mut last_cause = String::from("unknown");
                    let mut ok = None;
                    for attempt in 1..=RETRIES {
                        match self.fetch_bytes(&url).await {
                            Ok(b) => {
                                ok = Some(b);
                                break;
                            }
                            Err(e) => {
                                last_cause = e.to_string();
                                if attempt < RETRIES {
                                    tokio::time::sleep(std::time::Duration::from_millis(
                                        500 * attempt as u64,
                                    ))
                                    .await;
                                }
                            }
                        }
                    }
                    ok.ok_or_else(|| DownloadError::Failed {
                        file_name: item.file_name.clone(),
                        attempts: RETRIES,
                        cause: last_cause,
                    })?
                }
            };
            if let Some(expect) = &item.sha1 {
                let got = sha1_hex(&bytes);
                if !got.eq_ignore_ascii_case(expect) {
                    return Err(DownloadError::Failed {
                        file_name: item.file_name.clone(),
                        attempts: 0,
                        cause: format!("sha1 校验不一致（期望 {expect} · 实际 {got}）"),
                    });
                }
            }
            if let Some(parent) = cache.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&cache, &bytes)?;
        }
        // 缓存 → 目标位置
        if let Some(parent) = item.dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(&cache, &item.dest).map_err(|e| DownloadError::Failed {
            file_name: item.file_name.clone(),
            attempts: 0,
            cause: format!("{e}（dest={}）", item.dest.display()),
        })?;
        Ok(())
    }

    /// 缓存文件与声明 sha1 是否一致（未声明视为可用；读取失败按不一致处理，触发重取）
    fn verify_cache(&self, item: &ItemSpec, cache: &Path) -> bool {
        let Some(expect) = &item.sha1 else {
            return true;
        };
        let bytes = std::fs::read(cache).unwrap_or_default();
        sha1_hex(&bytes).eq_ignore_ascii_case(expect)
    }

    /// Maven 系 URL 带伴生 `<jar>.sha1` 文本：为无校验值的条目 best-effort 补上
    /// （Fabric meta 的 server/jar 端点无伴生文件，取不到则保持不校验）
    pub async fn attach_side_sha1(&self, spec: &mut ItemSpec) {
        if spec.sha1.is_some() {
            return;
        }
        let Fetch::Url(url) = &spec.fetch else { return };
        let Ok(bytes) = self.fetch_bytes(&format!("{url}.sha1")).await else {
            return;
        };
        let text = String::from_utf8_lossy(&bytes).trim().to_ascii_lowercase();
        if text.len() == 40 && text.chars().all(|c| c.is_ascii_hexdigit()) {
            spec.sha1 = Some(text);
        }
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
        facets.push(vec![format!("categories:{}", loader_cat(q.loader))]);
        if let Some(cat) = &q.category {
            if !cat.is_empty() && cat != "all" {
                facets.push(vec![format!("categories:{cat}")]);
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

/// 读取本地 zip 包内条目字节
fn read_zip_entry(archive: &Path, entry: &str) -> Result<Vec<u8>, DownloadError> {
    use std::io::Read;
    let f = File::open(archive)?;
    let mut z = zip::ZipArchive::new(f).map_err(|e| DownloadError::Failed {
        file_name: entry.to_string(),
        attempts: 0,
        cause: format!("zip 读取失败：{e}"),
    })?;
    let mut rf = z.by_name(entry).map_err(|e| DownloadError::Failed {
        file_name: entry.to_string(),
        attempts: 0,
        cause: format!("包内条目缺失：{e}"),
    })?;
    let mut buf = Vec::new();
    rf.read_to_end(&mut buf)?;
    Ok(buf)
}
