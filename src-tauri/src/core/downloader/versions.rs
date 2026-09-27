//! 版本表与加载器 jar 坐标：piston-meta 的 MC 版本、Fabric/Forge/NeoForge 三张表，
//! 外加「某 MC 版本要哪档 Java」那条官方字段的取数与本地表（`java-index.json`）。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::models::{LoaderKind, VersionOption};
use super::client::Downloader;
use super::types::{DownloadError, Fetch, ItemSpec};
use super::util::version_cmp;

const PISTON_MANIFEST: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const FABRIC_META: &str = "https://meta.fabricmc.net/v2";
const FORGE_PROMOTIONS: &str = "https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json";
/// 每个 MC 版本的全部 Forge 构建：{ "1.20.1": ["1.20.1-47.4.10", …], … }
const FORGE_MAVEN_META: &str = "https://files.minecraftforge.net/net/minecraftforge/forge/maven-metadata.json";
const NEOFORGE_VERSIONS: &str =
    "https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/neoforge";
/// `cache_dir/java-index.json`：MC 版本号 → 官方 `javaVersion.majorVersion`。
/// 与 `env-index.json` 同级，也就是躺在 `files\` 与 `tasks\` 这两个可清理目录**之外** ⇒ 设置页
/// 那张缓存卡看不见它（它不是「越攒越大的那类东西」：一条一个整数，全官方版本封顶一千来条）。
/// 存的是不可变事实——某个 MC 版本要哪档 Java 永远不会改 ⇒ 命中即用，不设 TTL。
const JAVA_INDEX_FILE: &str = "java-index.json";

impl Downloader {
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

    /// 某个 MC 版本要求的 Java 主版本 —— 拿**官方权威字段** `javaVersion.majorVersion`
    /// （piston-meta 每条版本记录的 `url` 指向那份 JSON 里；官方启动器就是按它挑 JRE 的）。
    ///
    /// 命中本地表即离线可答；没命中才要两趟请求——piston-meta 没有「按 id 直取版本 JSON」的端点，
    /// 那个 packages URL 只能从清单里查（清单那一趟有 BMCLAPI 镜像，第二趟**没有**：
    /// `source::PREFIXES` 只重写 `piston-meta.mojang.com/mc/`，而 `/v1/packages/…json` 实测
    /// HEAD 200 / GET 302 / 跟随后 TLS 握手失败，所以不往里加映射）。
    ///
    /// 任何一步取不到都返回 `None`，由命令层回落到 `java::required_for_mc` 那张表：转换要能在
    /// 完全断网时做，需求线不能是「查不到就没有」。失败也不写表（只缓存查到过的答案）。
    pub async fn java_major_official(&self, cache_dir: &Path, mc: &str) -> Option<u32> {
        let id = mc.trim();
        if id.is_empty() {
            return None;
        }
        if let Some(major) = java_index_load(cache_dir).get(id).copied() {
            return Some(major);
        }
        let list = self.get_json(PISTON_MANIFEST).await.ok()?;
        let url = list["versions"]
            .as_array()?
            .iter()
            .find(|e| e["id"].as_str() == Some(id))?["url"]
            .as_str()?
            .to_string();
        let v = self.get_json(&url).await.ok()?;
        let major = v["javaVersion"]["majorVersion"].as_u64()?;
        // 荒谬值不当答案用：官方字段读歪一格就把闸门钉死，不如不读（实测全量正式版落在 8…25）
        if !matches!(major, 8..=99) {
            return None;
        }
        let major = major as u32;
        java_index_put(cache_dir, id, major);
        Some(major)
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

    /// Fabric 官方服务端 jar。名字里的「一体化」是误称：实测它是 Fabric installer
    /// （内容全是 `net.fabricmc.installer.*`，`Main-Class: ServerLauncher`），
    /// 首次启动才现拉 libraries + intermediary + loader 并解出 vanilla 服务端 ⇒ 并非离线可跑
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

/// 读那张裸表：文件不在/坏一个字符都给空表，不给错误——这条链上「查不到」是常态而非失败
fn java_index_load(cache_dir: &Path) -> BTreeMap<String, u32> {
    std::fs::read_to_string(cache_dir.join(JAVA_INDEX_FILE))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// 写一条并落盘。写前重新读一遍再并：两条命令同时在飞时不该互相抹掉对方那条
/// （掉一条不毁正确性，只是下次多要一次清单，所以这里不引锁）
fn java_index_put(cache_dir: &Path, id: &str, major: u32) {
    let mut index = java_index_load(cache_dir);
    index.insert(id.to_string(), major);
    // 盘上就是这张表本身。env-index 那条教训：套一层 `{"map": …}` 而读侧按裸表解，
    // 写进去的答案就永远读不出来，每一轮都从零发请求
    if let Ok(json) = serde_json::to_string(&index) {
        let _ = std::fs::create_dir_all(cache_dir);
        let _ = std::fs::write(cache_dir.join(JAVA_INDEX_FILE), json);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ss-java-index-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or_default()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 落盘形状必须是裸表：外层套壳 ⇒ 读侧解不出来 ⇒ 缓存形同虚设（env-index 踩过的那一格）
    #[test]
    fn the_index_lands_on_disk_as_a_bare_map() {
        let dir = tmp();
        java_index_put(&dir, "26.3", 25);
        java_index_put(&dir, "1.20.1", 17);
        let text = std::fs::read_to_string(dir.join(JAVA_INDEX_FILE)).unwrap();
        assert!(text.starts_with("{\"1.20.1\":17,\"26.3\":25}"), "{text}");
        assert_eq!(java_index_load(&dir).get("26.3"), Some(&25));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 坏文件、缺文件都当空表用：这条链的兜底是那张表，不是报错
    #[test]
    fn an_unreadable_index_reads_as_empty() {
        let dir = tmp();
        std::fs::write(dir.join(JAVA_INDEX_FILE), b"{ not json").unwrap();
        assert!(java_index_load(&dir).is_empty());
        java_index_put(&dir, "1.21.1", 21);
        assert_eq!(java_index_load(&dir).get("1.21.1"), Some(&21));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 命中本地表 ⇒ 一趟请求都不发（这条是「离线转换也拿得到答案」的判据）
    #[tokio::test]
    async fn a_cached_answer_needs_no_request() {
        let dir = tmp();
        java_index_put(&dir, "1.20.1", 17);
        // 缓存目录指过去就够：这里没有网络，命中即返回；漏了命中会去打 piston-meta 而挂掉
        let d = Downloader::new(dir.clone(), 1);
        assert_eq!(d.java_major_official(&dir, "1.20.1").await, Some(17));
        assert_eq!(d.java_major_official(&dir, "  ").await, None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
