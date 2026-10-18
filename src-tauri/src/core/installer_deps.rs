//! 本机安装前的依赖清单：读安装器 jar 里那两份 profile，认出「安装器会自己去远程取的那些文件」。
//! 为什么要我们自己读：那几十上百发请求出自安装器子进程，它既不认设置里的下载源、也不认系统代理，
//! 而 `maven.neoforged.net` 在简中网络是被 DNS 投毒的 ⇒ 取数收回自己手里，档位、缓存、sha1、回落才一并生效。
//! - **两份 JSON 都得读**：`version.json`（启动器 profile）另有 29～47 条，只读 `install_profile.json`
//!   会漏 `cpw.mods:bootstraplauncher`，实测安装器退出码 1（PCL 与 PrismLauncher 同样读两份）。
//! - 落位相对路径取 `downloads.*.path`，没有才从 URL 推（剥掉仓库前缀那一层）。
//! - jar 内 `maven/<路径>` 是内嵌件（实测老 Forge 有 2 枚），安装器直接解出来、不发请求 ⇒ 不列进来。
//! - 认不出的形状一律回空表：预置是加速件，缺了安装器照旧自己取，不能因为解析不了就把任务判死。

use std::collections::{BTreeMap, HashSet};
use std::fs::File;
use std::io::Read;
use std::path::Path;

use serde_json::Value;

/// 安装器要远程取的枚一枚依赖。`rel` 是它在 `libraries/` 下的相对路径（maven 坐标展开后的 POSIX 形态）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepFile {
    pub rel: String,
    pub url: String,
    /// 安装器给的校验值：既是我们下载侧的校验锚，也让安装器认出「这枚已经在、不用取」
    pub sha1: Option<String>,
    pub size_bytes: u64,
}

/// 两份 profile 都在 jar 根上，名字固定（实测 NeoForge 21.1 / Forge 1.20.1 / Forge 1.16.5 三家一致）
const PROFILES: [&str; 2] = ["install_profile.json", "version.json"];

/// 列出这一枚安装器 jar 需要远程取回的文件；读不出（不是 zip、没有 profile、形状不认识）就回空表。
/// 顺序按相对路径排序，为的是日志与单测可复现。
pub fn list(installer_jar: &Path) -> Vec<DepFile> {
    let Ok(file) = File::open(installer_jar) else {
        return Vec::new();
    };
    let Ok(mut zip) = zip::ZipArchive::new(file) else {
        return Vec::new();
    };
    let embedded = embedded_paths(&mut zip);
    let mut out: BTreeMap<String, DepFile> = BTreeMap::new();
    for name in PROFILES {
        let Some(json) = read_entry(&mut zip, name) else {
            continue;
        };
        // 单份解析失败只丢那一份：另一份能读出来就仍然预置（安装器少几个文件会自己补，全丢才是白等一趟）
        let Ok(profile) = serde_json::from_str::<Value>(&json) else {
            continue;
        };
        collect(&profile, &embedded, &mut out);
    }
    out.into_values().collect()
}

/// jar 自带的 `maven/…` 那部分：安装器解出来就用，不联网
fn embedded_paths<R: std::io::Read + std::io::Seek>(
    zip: &mut zip::ZipArchive<R>,
) -> HashSet<String> {
    zip.file_names()
        .filter_map(|n| n.strip_prefix("maven/"))
        .filter(|p| !p.is_empty() && !p.ends_with('/'))
        .map(str::to_string)
        .collect()
}

fn read_entry<R: std::io::Read + std::io::Seek>(
    zip: &mut zip::ZipArchive<R>,
    name: &str,
) -> Option<String> {
    let mut entry = zip.by_name(name).ok()?;
    let mut buf = String::new();
    entry.read_to_string(&mut buf).ok()?;
    Some(buf)
}

fn collect(profile: &Value, embedded: &HashSet<String>, out: &mut BTreeMap<String, DepFile>) {
    let Some(libs) = profile.get("libraries").and_then(Value::as_array) else {
        return;
    };
    for lib in libs {
        let Some(dl) = lib.get("downloads").and_then(Value::as_object) else {
            continue;
        };
        push(dl.get("artifact"), embedded, out);
        // 分类符（sources / classifiers）在实测的四份 profile 里都没出现，但 Mojang 那份
        // 启动器 profile 的通用形状里有：漏一枚就是安装器退出码 1，多列几枚只是多存几个文件
        if let Some(cls) = dl.get("classifiers").and_then(Value::as_object) {
            for entry in cls.values() {
                push(Some(entry), embedded, out);
            }
        }
    }
}

fn push(dl: Option<&Value>, embedded: &HashSet<String>, out: &mut BTreeMap<String, DepFile>) {
    let Some(dl) = dl.and_then(Value::as_object) else {
        return;
    };
    let Some(url) = dl
        .get("url")
        .and_then(Value::as_str)
        .filter(|u| u.starts_with("http"))
    else {
        return;
    };
    let rel = match dl.get("path").and_then(Value::as_str) {
        Some(p) => p.trim_matches('/').to_string(),
        None => rel_from_url(url),
    };
    if rel.is_empty() || rel.contains("..") || embedded.contains(&rel) {
        return;
    }
    out.entry(rel.clone()).or_insert_with(|| DepFile {
        rel,
        url: url.to_string(),
        // 缓存键按这串当目录名（见 downloader::util::cache_path_for），大小写不一会各存一份
        sha1: dl.get("sha1").and_then(Value::as_str).map(|s| s.to_ascii_lowercase()),
        size_bytes: dl.get("size").and_then(Value::as_u64).unwrap_or(0),
    });
}

/// 没有 `path` 时从 URL 推 maven 相对路径：去掉协议与主机，再剥掉仓库前缀那一层。
/// 只认实测到的两种（`releases/` = NeoForge 官方、`maven/` = BMCLAPI 形态）；
/// Forge 与 Mojang 的 URL 本来就是 `/{group}/{artifact}/{ver}/{file}`，没有前缀可剥
fn rel_from_url(url: &str) -> String {
    let tail = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let path = tail.split('/').skip(1).collect::<Vec<_>>().join("/");
    let path = path.split(['?', '#']).next().unwrap_or(&path);
    path.strip_prefix("releases/")
        .or_else(|| path.strip_prefix("maven/"))
        .unwrap_or(path)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;
    use zip::write::SimpleFileOptions;

    /// 测试现场的安装器 jar：只放 profile 与内嵌件，别的条目这一层不关心
    fn jar(entries: &[(&str, &str)]) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "sideshift-installer-deps-{}.jar",
            uuid::Uuid::new_v4()
        ));
        let mut w = zip::ZipWriter::new(File::create(&path).unwrap());
        let opts = SimpleFileOptions::default();
        for (name, body) in entries {
            w.start_file(*name, opts).unwrap();
            w.write_all(body.as_bytes()).unwrap();
        }
        w.finish().unwrap();
        path
    }

    fn rels(deps: &[DepFile]) -> Vec<&str> {
        deps.iter().map(|d| d.rel.as_str()).collect()
    }

    #[test]
    fn both_profiles_are_read_and_paths_deduped() {
        // bootstraplauncher 只活在 version.json 里 —— 漏读那一份就是当初实测退出码 1 的那一枚
        let jar = jar(&[
            (
                "install_profile.json",
                r#"{"libraries":[
                    {"name":"org.ow2.asm:asm:9.3","downloads":{"artifact":{
                        "path":"org/ow2/asm/asm/9.3/asm-9.3.jar",
                        "url":"https://maven.neoforged.net/releases/org/ow2/asm/asm/9.3/asm-9.3.jar",
                        "sha1":"bb","size":77}},
                     "rules":[{"name":"allow"}]},
                    {"name":"net.neoforged:neoforge:21.1.77","downloads":{"artifact":{
                        "path":"net/neoforged/neoforge/21.1.77/neoforge-21.1.77-universal.jar",
                        "url":"https://maven.neoforged.net/releases/net/neoforged/neoforge/21.1.77/neoforge-21.1.77-universal.jar",
                        "sha1":"cc"}}}
                ]}"#,
            ),
            (
                "version.json",
                r#"{"libraries":[
                    {"name":"cpw.mods:bootstraplauncher:2.0.2","downloads":{"artifact":{
                        "path":"cpw/mods/bootstraplauncher/2.0.2/bootstraplauncher-2.0.2.jar",
                        "url":"https://libraries.minecraft.net/cpw/mods/bootstraplauncher/2.0.2/bootstraplauncher-2.0.2.jar",
                        "sha1":"dd","size":50000}},
                     "id":"cpw.mods:bootstraplauncher:2.0.2"},
                    {"name":"org.ow2.asm:asm:9.3","downloads":{"artifact":{
                        "path":"org/ow2/asm/asm/9.3/asm-9.3.jar",
                        "url":"https://maven.minecraftforge.net/org/ow2/asm/asm/9.3/asm-9.3.jar",
                        "sha1":"ee"}}}
                ]}"#,
            ),
            // 内嵌件：安装器自己解出来，不该再下一遍
            ("maven/net/neoforged/neoforge/21.1.77/neoforge-21.1.77-universal.jar", "x"),
        ]);
        let deps = list(&jar);
        assert_eq!(
            rels(&deps),
            vec![
                "cpw/mods/bootstraplauncher/2.0.2/bootstraplauncher-2.0.2.jar",
                "org/ow2/asm/asm/9.3/asm-9.3.jar",
            ]
        );
        // 同一路径两份各说一个 URL：留先读到的那份（install_profile 在前，与安装器的取数顺序同向）
        assert_eq!(deps[1].url, "https://maven.neoforged.net/releases/org/ow2/asm/asm/9.3/asm-9.3.jar");
        assert_eq!(deps[1].sha1.as_deref(), Some("bb"));
        assert_eq!(deps[1].size_bytes, 77);
        assert_eq!(deps[0].size_bytes, 50000);
        let _ = std::fs::remove_file(&jar);
    }

    #[test]
    fn missing_path_is_peeled_off_the_url() {
        let jar = jar(&[(
            "install_profile.json",
            r#"{"libraries":[
                {"name":"a:b:1","downloads":{"artifact":{"url":"https://maven.neoforged.net/releases/a/b/1/b-1.jar"}}},
                {"name":"c:d:1","downloads":{"artifact":{"url":"https://bmclapi2.bangbang93.com/maven/c/d/1/d-1.jar"}}},
                {"name":"e:f:1","downloads":{"artifact":{"url":"https://maven.minecraftforge.net/e/f/1/f-1.jar"}}}
            ]}"#,
        )]);
        assert_eq!(
            rels(&list(&jar)),
            vec!["a/b/1/b-1.jar", "c/d/1/d-1.jar", "e/f/1/f-1.jar"]
        );
        let _ = std::fs::remove_file(&jar);
    }

    #[test]
    fn classifiers_come_along() {
        let jar = jar(&[(
            "install_profile.json",
            r#"{"libraries":[{"name":"g:h:1","downloads":{
                "artifact":{"path":"g/h/1/h-1.jar","url":"https://x/g/h/1/h-1.jar"},
                "classifiers":{"sources":{"path":"g/h/1/h-1-sources.jar","url":"https://x/g/h/1/h-1-sources.jar","sha1":"AB"}}
            }}]}"#,
        )]);
        let deps = list(&jar);
        assert_eq!(rels(&deps), vec!["g/h/1/h-1-sources.jar", "g/h/1/h-1.jar"]);
        // 大小写归一：缓存目录名用它，混档会各存一份
        assert_eq!(deps[0].sha1.as_deref(), Some("ab"));
        let _ = std::fs::remove_file(&jar);
    }

    #[test]
    fn a_jar_without_profiles_seeds_nothing() {
        // Fabric 给的那枚是「安装器 + 启动器」合一的 server jar，里面没有这两份 profile ⇒ 不预置
        let jar = jar(&[("README.txt", "not an installer profile")]);
        assert!(list(&jar).is_empty());
        // 连 zip 都不是（路径写错、文件被删）也不能炸，空表交回上层照样能装
        assert!(list(Path::new("E:/definitely/not/here.jar")).is_empty());
        let _ = std::fs::remove_file(&jar);
    }

    #[test]
    fn relative_escape_is_refused() {
        // profile 里的 path 来自外部 jar：带 `..` 的条目不许变成「往 libraries 外面写」的入口
        let jar = jar(&[(
            "install_profile.json",
            r#"{"libraries":[{"name":"x:y:1","downloads":{"artifact":{
                "path":"../../windows/temp/y.jar","url":"https://x/y"}}}]}"#,
        )]);
        assert!(list(&jar).is_empty());
        let _ = std::fs::remove_file(&jar);
    }
}
