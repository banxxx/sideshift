//! 下载量预估：与 task_engine 3.1–3.3 的取件分类完全同源
//! （钉住 → 包内匹配 → 本地 jar → Modrinth 解析 → keepDirs 资源 → 加载器），
//! 但一个字节也不传输：大小取实测字段、缺失回落 HEAD Content-Length，缓存命中照扣。

use std::collections::HashSet;

use crate::core::detector;
use crate::core::downloader::{Downloader, Fetch, ItemSpec};
use crate::core::parser::{self, ParsedPack};
use crate::models::{ConversionOptions, DownloadEstimate, LoaderKind, ModDisposition, PlanMod};

fn url_of(fetch: &Fetch) -> Option<&str> {
    match fetch {
        Fetch::Url(u) => Some(u),
        _ => None,
    }
}

fn spec_of(fetch: Fetch, file_name: String, sha1: Option<String>, size_bytes: u64) -> ItemSpec {
    ItemSpec {
        fetch,
        file_name,
        sha1,
        dest: Default::default(),
        size_bytes,
    }
}

/// 「大小未知但必须联网」的条目求真实大小：实测字段优先，回落 HEAD；两处都拿不到记不完整
async fn true_size(dl: &Downloader, known: u64, url: &str, complete: &mut bool) -> u64 {
    if known > 0 {
        return known;
    }
    match dl.head_size(url).await {
        Some(s) => s,
        None => {
            *complete = false;
            0
        }
    }
}

pub async fn estimate(
    parsed: &ParsedPack,
    plan: &[PlanMod],
    options: &ConversionOptions,
    dl: &Downloader,
) -> DownloadEstimate {
    let mut out = DownloadEstimate {
        download_bytes: 0,
        from_pack_bytes: 0,
        complete: true,
    };
    let mut used: HashSet<usize> = HashSet::new();

    for row in plan
        .iter()
        .filter(|m| m.disposition != ModDisposition::Remove)
        // 与构建 3.0/3.1 一致：探明拿不到字节的那些不进产物，也就不进下载量。
        // 留着它们报出来的数字是「要下多少」，而实际下的是另一回事
        .filter(|m| !m.cf_blocked)
    {
        // 与构建 3.1 一致：钉住行最先匹配（用户所选版本 = 实际下载版本）
        if let Some(p) = &row.pinned {
            let spec = spec_of(
                Fetch::Url(p.url.clone()),
                p.file_name.clone(),
                p.sha1.clone(),
                row.size_bytes,
            );
            if !dl.is_cached(&spec) {
                // CurseForge 的直链带时效，存档里 url 恒空 → 预估阶段没有可 HEAD 的地址：
                // 只用行上的实测大小，拿不到才记不完整
                out.download_bytes += if p.url.is_empty() {
                    if row.size_bytes == 0 {
                        out.complete = false;
                    }
                    row.size_bytes
                } else {
                    true_size(dl, row.size_bytes, &p.url, &mut out.complete).await
                };
            }
            continue;
        }
        let matched = detector::match_pack_index(&parsed.mod_files, row, &used)
            .map(|i| (i, &parsed.mod_files[i]));
        if let Some((i, f)) = matched {
            used.insert(i);
            // CF 那一档（清单只给编号）：字节不在包里，直链带时效、构建期才取 ⇒ 这行**一定**是联网项。
            // 大小问补回来的元数据；没补到（离线 / CF 没答）就是真不知道，记 incomplete，
            // 与上面 pinned 那条「url 恒空只用实测大小」同一口径。缓存按 URL 记账，这一档对不上号，
            // 所以宁可多算一遍也不扣（少扣会低估，多扣只是让用户白看一眼数字）
            if f.cf.is_some() {
                if f.size_bytes == 0 {
                    out.complete = false;
                } else {
                    out.download_bytes += f.size_bytes;
                }
                continue;
            }
            // 与构建一致：物理在包内 → 直取；仅残缺条目回落 URL
            if f.in_pack || f.url.is_empty() {
                out.from_pack_bytes += f.size_bytes;
            } else {
                let spec = spec_of(
                    Fetch::Url(f.url.clone()),
                    f.file_name.clone(),
                    f.sha1.clone(),
                    f.size_bytes,
                );
                if !dl.is_cached(&spec) {
                    out.download_bytes +=
                        true_size(dl, f.size_bytes, &f.url, &mut out.complete).await;
                }
            }
            continue;
        }
        if let Some(lp) = &row.local_path {
            out.from_pack_bytes += std::fs::metadata(lp).map(|m| m.len()).unwrap_or(0);
            continue;
        }
        // 自动补齐等外部新增：解析真实构建与大小（downloader 内有结果，构建同样会命中）
        match dl
            .resolve_mod_file(&row.id, &options.mc_version, parsed.manifest.loader)
            .await
        {
            Ok(spec) => {
                if !dl.is_cached(&spec) {
                    if let Some(u) = url_of(&spec.fetch).map(str::to_string) {
                        out.download_bytes +=
                            true_size(dl, spec.size_bytes, &u, &mut out.complete).await;
                    }
                }
            }
            // 离线或源不可达：这行构建时仍会尝试下载，只是大小暂不可知
            Err(_) => out.complete = false,
        }
    }

    // 3.2 keepDirs / keepFiles 资源（与构建同一口径：目录走 `{path}/` 前缀，文件走精确全等；
    //  落位是「勾哪层剪哪层」，但预估只看字节、不看落位名，所以这里不碰 kept_rel）
    for f in &parsed.extra_files {
        let rel = f.path.replace('\\', "/");
        let logical = parsed.logical_rel(&rel);
        let lower = logical.to_lowercase();
        // 与构建 3.2 同一道硬闸：路径里出现被禁目录名的条目不计进预估
        if parser::keep_denied(&lower) {
            continue;
        }
        let keep = options
            .keep_dirs
            .iter()
            .any(|d| lower.starts_with(&format!("{}/", d.to_lowercase())))
            || options.keep_files.iter().any(|p| **p == lower);
        if !keep {
            continue;
        }
        if f.in_pack || f.url.is_empty() {
            out.from_pack_bytes += f.size_bytes;
        } else {
            let spec = spec_of(
                Fetch::Url(f.url.clone()),
                f.file_name.clone(),
                f.sha1.clone(),
                f.size_bytes,
            );
            if !dl.is_cached(&spec) {
                out.download_bytes += true_size(dl, f.size_bytes, &f.url, &mut out.complete).await;
            }
        }
    }

    // 3.3 加载器本体
    if options.loader_version.trim().is_empty() {
        // 未选版本构建必失败（前端已拦截），大小无从谈起
        out.complete = false;
    } else {
        let mut spec = match parsed.manifest.loader {
            LoaderKind::Fabric => dl
                .fabric_server_jar(&options.mc_version, &options.loader_version)
                .await
                .ok(),
            LoaderKind::Forge => {
                let mut s = dl.forge_installer(&options.mc_version, &options.loader_version);
                dl.attach_side_sha1(&mut s).await;
                Some(s)
            }
            LoaderKind::NeoForge => {
                let mut s = dl.neoforge_installer(&options.loader_version);
                dl.attach_side_sha1(&mut s).await;
                Some(s)
            }
        };
        match spec.as_mut() {
            Some(s) => {
                if !dl.is_cached(s) {
                    if let Some(u) = url_of(&s.fetch).map(str::to_string) {
                        out.download_bytes += true_size(dl, s.size_bytes, &u, &mut out.complete).await;
                    }
                }
            }
            None => out.complete = false,
        }
    }
    out
}
