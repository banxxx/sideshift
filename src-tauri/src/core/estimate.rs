//! 下载量预估：与 task_engine 3.1–3.3 的取件分类完全同源
//! （钉住 → 包内匹配 → 本地 jar → Modrinth 解析 → keepDirs 资源 → 加载器），
//! 但一个字节也不传输：大小取实测字段、缺失回落 HEAD Content-Length，缓存命中照扣。

use std::collections::HashSet;

use crate::core::detector::split_mod_file;
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
                out.download_bytes +=
                    true_size(dl, row.size_bytes, &p.url, &mut out.complete).await;
            }
            continue;
        }
        let matched = parsed
            .mod_files
            .iter()
            .enumerate()
            .find(|(i, f)| !used.contains(i) && split_mod_file(&f.file_name).0 == row.id);
        if let Some((i, f)) = matched {
            used.insert(i);
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

    // 3.2 keepDirs 资源（与构建同一前缀匹配）
    for f in &parsed.extra_files {
        let rel = f.path.replace('\\', "/");
        let logical = parser::logical_rel(&rel);
        let lower = logical.to_lowercase();
        let keep = options
            .keep_dirs
            .iter()
            .any(|d| lower.starts_with(&format!("{}/", d.to_lowercase())));
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
