//! 转换方案生成：按证据阶梯裁决（作者声明 > 包内元数据 > 平台反查 > 名称启发），
//! 名称关键字只在没有任何证据时兜底；输出 PlanMod[] 与计数。

use std::collections::HashSet;

use crate::core::env::EvidenceMap;
use crate::core::parser::{PackFile, ParsedPack};
use crate::models::{EnvSource, LoaderKind, ModDisposition, PlanCounts, PlanMod, SideFlag};

/// 客户端专属模组关键字（文件名小写子串匹配；仅在所有证据层都拿不到时兜底）
const CLIENT_ONLY_KEYWORDS: &[&str] = &[
    "sodium",
    "iris",
    "optifine",
    "replaymod",
    "xaero",
    "minimap",
    "journeymap",
    "litematica",
    "tweakeroo",
    "itemscroller",
    "shulker",
    "continuity",
    "citresewn",
    "animatica",
    "appleskin",
    "hwyla",
    "entityculling",
    "zoomify",
    "dynamic-fps",
    "dynamicfps",
    "skinlayers",
    "emotecraft",
    "idlefixtures",
    "debugify", // dev 工具，服务端无收益
];

/// 需人工确认（有服务端价值但默认不剔除）的关键字
const NEEDS_REVIEW_KEYWORDS: &[&str] = &["viafabricplus", "viaversion", "viaaprilfools"];

/// 证据裁决结果
#[derive(Debug, PartialEq, Eq)]
enum Verdict {
    /// 服务端不需要（客户端专属，或「客户端必需 + 服务端可选」）
    Strip,
    /// 服务端需要，或客户端非必需
    Keep,
    /// 没有任何端证据：默认保留（多留一个模组不会炸服，误删才会）
    NoEvidence,
}

/// 两侧支持度 → 处置。定案口径（用户拍板）：「客户端必需 / 服务端可选」默认剔除。
fn verdict(client: Option<SideFlag>, server: Option<SideFlag>) -> Verdict {
    match (client, server) {
        (_, Some(SideFlag::Unsupported)) => Verdict::Strip,
        (Some(SideFlag::Required), Some(SideFlag::Optional)) => Verdict::Strip,
        (_, Some(SideFlag::Required)) | (_, Some(SideFlag::Optional)) => Verdict::Keep,
        (Some(SideFlag::Required), None) => Verdict::Strip,
        _ => Verdict::NoEvidence,
    }
}

/// 名称兜底表是否命中（命令层统计「无证据行数」时共用同一张表，避免两处口径漂移）
pub fn name_heuristic_hit(file_name: &str) -> bool {
    let lower = file_name.to_lowercase();
    CLIENT_ONLY_KEYWORDS.iter().any(|k| lower.contains(k))
}

/// 生成转换方案。
/// - `strip_client_only=false`：只给证据、不自动剔除（全部保留），供「关掉自动」的场景
/// - `ev`：包内条目 → 端证据（jar 元数据 / Modrinth 反查 / 本地索引），键为条目路径；
///   mrpack 自带的 env 声明优先级更高，不在此表内（parser 已给出）
pub fn build_plan(
    parsed: &ParsedPack,
    strip_client_only: bool,
    ev: &EvidenceMap,
) -> Vec<PlanMod> {
    let loader_label = parsed.manifest.loader.as_label().to_string();
    let mut plan: Vec<PlanMod> = parsed
        .mod_files
        .iter()
        .map(|f| {
            let (id, version) = split_mod_file(&f.file_name);
            let lower = f.file_name.to_lowercase();
            let needs_review = NEEDS_REVIEW_KEYWORDS.iter().any(|k| lower.contains(k));

            // 证据阶梯：mrpack env（作者显式声明）> jar 内元数据 / 平台反查（env 表内已按序取优）
            // > 名称启发式 > 无证据
            let (client, server, source) = if f.env_server.is_some() || f.env_client.is_some() {
                (f.env_client, f.env_server, EnvSource::Mrpack)
            } else {
                match ev.get(&f.path) {
                    Some(e) => (e.client, e.server, e.source),
                    None => {
                        // 兜底：关键字命中 = 认定「客户端必需、服务端不支持」
                        if name_heuristic_hit(&f.file_name) {
                            (
                                Some(SideFlag::Required),
                                Some(SideFlag::Unsupported),
                                EnvSource::NameHeuristic,
                            )
                        } else {
                            (None, None, EnvSource::Unknown)
                        }
                    }
                }
            };

            let strip = verdict(client, server) == Verdict::Strip;
            let client_only = strip && strip_client_only;
            PlanMod {
                id,
                name: title_from_id(&lower_file_stem(&f.file_name)),
                version,
                loader: Some(loader_label.clone()),
                disposition: if client_only && !needs_review {
                    ModDisposition::Remove
                } else {
                    ModDisposition::Keep
                },
                client_only,
                needs_review,
                auto_supplement: false,
                size_bytes: f.size_bytes,
                // 只有「有 URL 可下且物理不在包内」才是真联网下载；
                // mrpack 正常条目全部内嵌 → 包内直取
                needs_download: !f.in_pack && !f.url.is_empty(),
                local_path: None,
                pinned: None,
                depends: Vec::new(),
                src_path: Some(f.path.clone()),
                env_source: source,
                client_side: client,
                server_side: server,
            }
        })
        .collect();

    // mrpack files[].depends 引用的是 Modrinth project_id，映射回方案行 id：
    // 行 id 由文件名切出（如 "sodium-fabric"），故按 精确 > 前缀 匹配首个宿主行
    for (i, f) in parsed.mod_files.iter().enumerate() {
        let mut deps = Vec::new();
        for p in &f.depends {
            if let Some(j) = plan.iter().position(|m| &m.id == p).or_else(|| {
                plan.iter().position(|m| {
                    m.id.len() > p.len() && m.id.starts_with(&format!("{p}-"))
                })
            }) {
                if j != i && !deps.contains(&plan[j].id) {
                    deps.push(plan[j].id.clone());
                }
            }
        }
        plan[i].depends = deps;
    }

    // 依赖图保护：被保留行硬依赖的模组即便被判为客户端专属也不剔——
    // 多半是误判的通用库（如某些库被 Fabric 模组声明依赖却自身标 client）。
    // 强制保留后标「待人工确认」，让人看得见这台保险。
    loop {
        let needed: HashSet<String> = plan
            .iter()
            .filter(|m| m.disposition == ModDisposition::Keep)
            .flat_map(|m| m.depends.iter().map(|d| d.to_string()))
            .collect();
        let mut changed = false;
        for m in plan.iter_mut() {
            if m.disposition == ModDisposition::Remove && needed.contains(m.id.as_str()) {
                m.disposition = ModDisposition::Keep;
                m.needs_review = true;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    // 自动补齐：Fabric 包缺 fabric-api 时补服务端基础库
    let ids: Vec<String> = plan.iter().map(|m| m.id.to_lowercase()).collect();
    let has = |kw: &str| ids.iter().any(|i| i.contains(kw));
    if parsed.manifest.loader == LoaderKind::Fabric && !has("fabric-api") && !has("api-fabric") {
        plan.push(PlanMod {
            id: "fabric-api".into(),
            name: "Fabric API".into(),
            version: String::new(),
            loader: Some(loader_label.clone()),
            disposition: ModDisposition::Add,
            client_only: false,
            needs_review: false,
            auto_supplement: true,
            size_bytes: 0,
            needs_download: true,
            local_path: None,
            pinned: None,
            depends: Vec::new(),
            src_path: None,
            env_source: EnvSource::Unknown,
            client_side: None,
            server_side: Some(SideFlag::Required),
        });
    }
    // 推荐项：服务端性能监控 spark
    if !has("spark") {
        plan.push(PlanMod {
            id: "spark".into(),
            name: "spark".into(),
            version: String::new(),
            loader: Some(loader_label.clone()),
            disposition: ModDisposition::Add,
            client_only: false,
            needs_review: false,
            auto_supplement: false,
            size_bytes: 0,
            needs_download: true,
            local_path: None,
            pinned: None,
            depends: Vec::new(),
            src_path: None,
            env_source: EnvSource::Unknown,
            client_side: None,
            server_side: Some(SideFlag::Optional),
        });
    }
    plan
}

/// 方案行 → 包内条目下标：优先 src_path 精确锚定（同一 id 有多个文件时防张冠李戴），
/// 回落「按文件名切 id + 先到先得」。构建 3.1 与预估共用，保证两侧取到同一个条目。
pub fn match_pack_index(
    files: &[PackFile],
    row: &PlanMod,
    used: &HashSet<usize>,
) -> Option<usize> {
    if let Some(p) = &row.src_path {
        if let Some(i) = files
            .iter()
            .enumerate()
            .position(|(i, f)| !used.contains(&i) && &f.path == p)
        {
            return Some(i);
        }
    }
    files
        .iter()
        .enumerate()
        .position(|(i, f)| !used.contains(&i) && split_mod_file(&f.file_name).0 == row.id)
}

pub fn count_plan(plan: &[PlanMod]) -> PlanCounts {
    PlanCounts {
        remove: plan.iter().filter(|m| m.disposition == ModDisposition::Remove).count() as u32,
        keep: plan.iter().filter(|m| m.disposition == ModDisposition::Keep).count() as u32,
        add: plan.iter().filter(|m| m.disposition == ModDisposition::Add).count() as u32,
    }
}

/// "fabric-api-0.92.2+1.20.1.jar" → ("fabric-api", "0.92.2+1.20.1")
pub fn split_mod_file(file_name: &str) -> (String, String) {
    let stem = file_name
        .strip_suffix(".jar")
        .or_else(|| file_name.strip_suffix(".JAR"))
        .unwrap_or(file_name);
    let bytes = stem.as_bytes();
    for i in 0..bytes.len() {
        if bytes[i] != b'-' {
            continue;
        }
        let rest = &stem[i + 1..];
        let rb = rest.as_bytes();
        let version_like = rb.first().is_some_and(|c| c.is_ascii_digit())
            || (rb.first() == Some(&b'v') && rb.get(1).is_some_and(|c| c.is_ascii_digit()));
        if version_like {
            return (stem[..i].to_string(), rest.to_string());
        }
    }
    (stem.to_string(), String::new())
}

fn lower_file_stem(file_name: &str) -> String {
    file_name
        .strip_suffix(".jar")
        .unwrap_or(file_name)
        .to_lowercase()
}

/// "fabric-api-0.92.2" → 取 '-' 前模组名部分并词首大写："Fabric Api"
fn title_from_id(stem: &str) -> String {
    let id_part = stem
        .split('-')
        .take_while(|t| !t.chars().next().is_some_and(|c| c.is_ascii_digit()))
        .collect::<Vec<_>>()
        .join("-");
    id_part
        .split(['-', '_'])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_mod_file_basics() {
        assert_eq!(
            split_mod_file("fabric-api-0.92.2+1.20.1.jar"),
            ("fabric-api".into(), "0.92.2+1.20.1".into())
        );
        assert_eq!(
            split_mod_file("sodium-0.4.2.jar"),
            ("sodium".into(), "0.4.2".into())
        );
        assert_eq!(split_mod_file("jei.jar"), ("jei".into(), "".into()));
        assert_eq!(
            title_from_id("viafabricplus-0.2.3"),
            "Viafabricplus"
        );
    }

    #[test]
    fn depends_project_id_maps_to_plan_row() {
        use crate::core::parser::{PackFile, ParsedPack};
        use crate::models::PackManifest;
        let file = |name: &str, depends: &[&str]| PackFile {
            path: format!("mods/{name}"),
            file_name: name.into(),
            url: String::new(),
            sha1: None,
            size_bytes: 0,
            in_pack: true,
            env_server: Some(SideFlag::Required),
            env_client: None,
            depends: depends.iter().map(|s| s.to_string()).collect(),
        };
        let parsed = ParsedPack {
            manifest: PackManifest {
                file_name: "t.mrpack".into(),
                loader: LoaderKind::NeoForge, // 避开 Fabric 自动补齐行的干扰
                mc_version: "1.20.1".into(),
                mod_count: 2,
                size_bytes: 0,
                parsed: true,
                error: None,
                source_path: None,
            },
            mod_files: vec![
                file("geckolib-4.4.7.jar", &[]),
                file("create-0.5.1.jar", &["geckolib", "sodium"]),
            ],
            extra_files: Vec::new(),
            loader_version: None,
        };
        let plan = build_plan(&parsed, true, &EvidenceMap::new());
        // project_id 精确命中行 id；包内不存在的 "sodium" 被丢弃
        assert_eq!(plan[1].depends, vec!["geckolib".to_string()]);
    }

    /* ---------------- 端证据裁决 ---------------- */

    use crate::core::env::Evidence;
    use crate::core::parser::ParsedPack;
    use crate::models::PackManifest;

    /// NeoForge 包（避开 Fabric 的 fabric-api 自动补行干扰），条目默认无 env 声明
    fn pack(files: Vec<PackFile>) -> ParsedPack {
        ParsedPack {
            manifest: PackManifest {
                file_name: "t.mrpack".into(),
                loader: LoaderKind::NeoForge,
                mc_version: "1.20.1".into(),
                mod_count: files.len() as u32,
                size_bytes: 0,
                parsed: true,
                error: None,
                source_path: None,
            },
            mod_files: files,
            extra_files: Vec::new(),
            loader_version: None,
        }
    }

    fn file(name: &str, env: (Option<SideFlag>, Option<SideFlag>)) -> PackFile {
        PackFile {
            path: format!("mods/{name}"),
            file_name: name.into(),
            url: String::new(),
            sha1: None,
            size_bytes: 0,
            in_pack: true,
            env_server: env.0,
            env_client: env.1,
            depends: Vec::new(),
        }
    }

    fn add(
        map: &mut EvidenceMap,
        path: &str,
        client: SideFlag,
        server: SideFlag,
        source: EnvSource,
    ) {
        map.insert(
            path.to_string(),
            Evidence {
                client: Some(client),
                server: Some(server),
                source,
            },
        );
    }

    #[test]
    fn mrpack_declares_client_only_row_as_removed() {
        let parsed = pack(vec![file(
            "sodium-0.5.13.jar",
            (Some(SideFlag::Unsupported), Some(SideFlag::Required)),
        )]);
        let plan = build_plan(&parsed, true, &EvidenceMap::new());
        assert_eq!(plan[0].disposition, ModDisposition::Remove);
        assert!(plan[0].client_only);
        assert_eq!(plan[0].env_source, EnvSource::Mrpack);
    }

    #[test]
    fn client_required_server_optional_is_removed_by_default() {
        // 用户拍板口径：「客户端必需 / 服务端可选」默认剔除（如 Xaero 小地图）
        let parsed = pack(vec![file("xaeros-world-map-1.0.jar", (None, None))]);
        let mut map = EvidenceMap::new();
        add(
            &mut map,
            "mods/xaeros-world-map-1.0.jar",
            SideFlag::Required,
            SideFlag::Optional,
            EnvSource::ModrinthHash,
        );
        let plan = build_plan(&parsed, true, &map);
        assert_eq!(plan[0].disposition, ModDisposition::Remove);
        assert_eq!(plan[0].env_source, EnvSource::ModrinthHash);
    }

    #[test]
    fn prefers_both_and_server_required_stay_kept() {
        let parsed = pack(vec![file("jei-1.0.jar", (None, None)), file("luckperms-1.0.jar", (None, None))]);
        let mut map = EvidenceMap::new();
        add(
            &mut map,
            "mods/jei-1.0.jar",
            SideFlag::Optional,
            SideFlag::Optional,
            EnvSource::JarMetadata,
        );
        add(
            &mut map,
            "mods/luckperms-1.0.jar",
            SideFlag::Unsupported,
            SideFlag::Required,
            EnvSource::ModrinthProject,
        );
        let plan = build_plan(&parsed, true, &map);
        assert_eq!(plan[0].disposition, ModDisposition::Keep);
        assert_eq!(plan[1].disposition, ModDisposition::Keep);
    }

    #[test]
    fn no_evidence_keeps_row_without_review_flag() {
        // 未判定 = 保留且不制造「待人工确认」噪音（多留模组不炸服，误删才会）
        let parsed = pack(vec![file("some-obscure-lib-1.0.jar", (None, None))]);
        let plan = build_plan(&parsed, true, &EvidenceMap::new());
        assert_eq!(plan[0].disposition, ModDisposition::Keep);
        assert!(!plan[0].needs_review);
        assert_eq!(plan[0].env_source, EnvSource::Unknown);
    }

    #[test]
    fn name_heuristic_is_last_resort_and_labels_its_source() {
        let parsed = pack(vec![file("continuity-3.0.jar", (None, None))]);
        let plan = build_plan(&parsed, true, &EvidenceMap::new());
        assert_eq!(plan[0].disposition, ModDisposition::Remove);
        assert_eq!(plan[0].env_source, EnvSource::NameHeuristic);
    }

    #[test]
    fn strip_switch_off_keeps_everything_but_keeps_the_evidence() {
        let parsed = pack(vec![file(
            "sodium-0.5.13.jar",
            (Some(SideFlag::Unsupported), Some(SideFlag::Required)),
        )]);
        let plan = build_plan(&parsed, false, &EvidenceMap::new());
        assert_eq!(plan[0].disposition, ModDisposition::Keep);
        assert!(!plan[0].client_only);
        assert_eq!(plan[0].server_side, Some(SideFlag::Unsupported));
    }

    #[test]
    fn hard_dependency_rescues_a_stripped_library_and_flags_review() {
        let mut create = file(
            "create-0.5.1.jar",
            (Some(SideFlag::Required), Some(SideFlag::Required)),
        );
        create.depends = vec!["geckolib".into()];
        let parsed = pack(vec![
            file(
                "geckolib-4.4.7.jar",
                (Some(SideFlag::Unsupported), Some(SideFlag::Required)),
            ),
            create,
        ]);
        // geckolib 被作者标成 client-only（env: server=unsupported）→ 本应剔除
        let plan = build_plan(&parsed, true, &EvidenceMap::new());
        assert_eq!(plan[0].disposition, ModDisposition::Keep);
        assert!(plan[0].needs_review, "被保留行硬依赖，应强制保留并标待确认");
    }
}
