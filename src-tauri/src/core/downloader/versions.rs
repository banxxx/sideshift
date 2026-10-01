//! 版本表与加载器 jar 坐标：piston-meta 的 MC 版本、Fabric/Forge/NeoForge 三张表，
//! 外加「某 MC 版本要哪档 Java」那条官方字段的取数与本地表（`java-index.json`）。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::core::mc_version;
use crate::models::{LoaderKind, VersionOption};
use super::client::{Downloader, OnceTable};
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

/// MC 版本清单：piston-meta 一次全量返回，内容周级才动 ⇒ 进程内问一次就够（规矩见 `OnceTable`）
static MC_VERSIONS: OnceTable<VersionOption> = OnceTable::new();

impl Downloader {
    /// 全量版本清单（接口本就一次返回，不截断，前端搜索即全量过滤）。
    /// 收 正式版 + Beta + Alpha 以兼容老整合包；快照（400+ 条）不进选择器。
    pub async fn list_mc_versions(&self) -> Result<Vec<VersionOption>, DownloadError> {
        MC_VERSIONS
            .memo(|| async move {
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
            })
            .await
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
                Ok(fabric_loader_options(&v))
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
                let mut out = Vec::new();
                let prefix = neoforge_prefix(mc_version);
                if let (Some(prefix), Some(arr)) = (prefix, v["versions"].as_array()) {
                    let mut vers: Vec<String> = arr
                        .iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .filter(|s| s.starts_with(prefix.as_str()))
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

/// Fabric meta `/versions/loader/{mc}` 的响应 → 下拉候选。
///
/// 不按 `loader.stable` 过滤（2026-09-28 实测否掉了旧口径）：`loader.stable` 是**这款 loader 全局**
/// 的稳定标记，全平台只有最新那一枚为 true，而接口的 MC 过滤对近版本几乎不生效——
/// 1.16.5 / 1.17.1 / 1.20.1 / 1.20.6 / 1.21.1 五档都返回 253 条、集合逐条相同。
/// 旧口径把 253 条压成 1 条，等于下拉里只有一个号、且换 MC 版本也不变。
/// 顺序照官方响应（新→旧），推荐位落在第一枚 stable 上（没有 stable 就落首条）。
fn fabric_loader_options(v: &serde_json::Value) -> Vec<VersionOption> {
    let mut out: Vec<VersionOption> = Vec::new();
    let mut rec: Option<usize> = None;
    if let Some(arr) = v.as_array() {
        for e in arr {
            let Some(ver) = e["loader"]["version"].as_str() else {
                continue;
            };
            if ver.is_empty() || out.iter().any(|o| o.value == ver) {
                continue;
            }
            if rec.is_none() && e["loader"]["stable"].as_bool() == Some(true) {
                rec = Some(out.len());
            }
            out.push(VersionOption {
                value: ver.to_string(),
                label: ver.to_string(),
                recommended: None,
                group: None,
            });
        }
    }
    if let Some(slot) = out.get_mut(rec.unwrap_or(0)) {
        slot.recommended = Some(true);
    }
    out
}

/// MC 版本 → NeoForge 版本号的公共前缀。NeoForge 版本号的前两段就是它对应 MC 的
/// 「版本线.补丁号」（`21.1.3` ↔ MC 1.21.1、`26.2.0.1-beta` ↔ MC 26.2），偏移恒为 0，
/// 两套 MC 写法都从 `mc_version::parse` 出（`1.21.1` → `21.1.`、`26.2` → `26.2.`）。
/// 只比主版本不够：那会把整个 1.21.x 灌进 1.21.1 的下拉。尾点也不能省：`21.1` 会多吞 108 条
/// `21.10.x`，带上点才是那一档；实测全表 1733 条段数都 ≥3，所以带尾点不会漏掉两段名。
/// 解析不出来就不给候选——这张表没有「兜底猜一档」的意义，猜错的代次比空列表更贵
/// （installer 只认自己那份 version JSON，装得成功但做出的是别的 MC 版本的服）。
fn neoforge_prefix(mc: &str) -> Option<String> {
    let l = mc_version::parse(mc)?;
    Some(format!("{}.{}.", l.line, l.patch))
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

    /// NeoForge 的前缀 = 「MC 版本线.补丁号.」，两套编号都吃这一条（号码表 2026-09-27 实测）。
    /// 尾点不能省：`21.1.` 挡得住 `21.10.x` 混进 1.21.1 的下拉。
    #[test]
    fn neoforge_prefix_reads_both_numbering_schemes() {
        assert_eq!(neoforge_prefix("1.20.4").as_deref(), Some("20.4."));
        assert_eq!(neoforge_prefix("1.21.1").as_deref(), Some("21.1."));
        assert_eq!(neoforge_prefix("1.20").as_deref(), Some("20.0."));
        assert_eq!(neoforge_prefix("26.1").as_deref(), Some("26.1."));
        assert_eq!(neoforge_prefix("26.3").as_deref(), Some("26.3."));
        // 认不出的串给 None ⇒ 空列表，而不是把全表端出去
        assert_eq!(neoforge_prefix("24w14a"), None);
        assert_eq!(neoforge_prefix(""), None);
    }

    /// Fabric 的候选＝整份响应，不是「只留 stable 那一枚」。
    /// 形状照 2026-09-28 实测：`/versions/loader/1.20.1` 返回 253 条，`loader.stable` 全表只有
    /// 首条（0.19.5）为 true ⇒ 按 stable 过滤会把下拉压成一个号，换 MC 版本也看不出演变。
    #[test]
    fn fabric_candidates_keep_every_version_and_mark_the_stable_one() {
        let v = serde_json::json!([
            { "loader": { "version": "0.19.5", "stable": true } },
            { "loader": { "version": "0.19.4", "stable": false } },
            { "loader": { "version": "0.15.3", "stable": false } },
            { "loader": { "version": "", "stable": false } },
            { "loader": { "version": "0.15.3", "stable": false } },
            { "instantiator": {} },
        ]);
        let out = fabric_loader_options(&v);
        // 空号与重复号都不进列表
        assert_eq!(
            out.iter().map(|o| o.value.as_str()).collect::<Vec<_>>(),
            ["0.19.5", "0.19.4", "0.15.3"]
        );
        // 推荐位是 stable 那一枚，不是首条之外的位置；其余都不带推荐
        let rec: Vec<&str> = out
            .iter()
            .filter(|o| o.recommended == Some(true))
            .map(|o| o.value.as_str())
            .collect();
        assert_eq!(rec, ["0.19.5"]);
        // 全表没有 stable ⇒ 回落首条，推荐位不能整份缺席
        let none_stable = serde_json::json!([
            { "loader": { "version": "0.16.9", "stable": false } },
            { "loader": { "version": "0.16.5", "stable": false } },
        ]);
        let out = fabric_loader_options(&none_stable);
        assert_eq!(out[0].recommended, Some(true));
        assert_eq!(out[1].recommended, None);
        // 空响应给空列表（源包声明的那一档由前端兜着，这里不硬造号）
        assert!(fabric_loader_options(&serde_json::json!([])).is_empty());
    }
}
