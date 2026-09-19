//! 转换方案生成：env 元数据优先，模组名启发式兜底；输出 PlanMod[] 与计数。

use crate::core::parser::ParsedPack;
use crate::models::{LoaderKind, ModDisposition, PlanCounts, PlanMod};

/// 客户端专属模组关键字（文件名小写子串匹配；仅在包内无 env 元数据时兜底）
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

/// 生成转换方案。strip_client_only=false 时不自动剔除客户端模组（全部保留）。
pub fn build_plan(parsed: &ParsedPack, strip_client_only: bool) -> Vec<PlanMod> {
    let loader_label = parsed.manifest.loader.as_label().to_string();
    let mut plan: Vec<PlanMod> = parsed
        .mod_files
        .iter()
        .map(|f| {
            let (id, version) = split_mod_file(&f.file_name);
            let lower = f.file_name.to_lowercase();
            let needs_review = NEEDS_REVIEW_KEYWORDS.iter().any(|k| lower.contains(k));
            // env 元数据优先（mrpack 精确声明）；无 env 时按模组名启发式
            let client_only = if f.env_declared {
                !f.server_required
            } else {
                CLIENT_ONLY_KEYWORDS.iter().any(|k| lower.contains(k))
            };
            let disposition = if needs_review {
                ModDisposition::Keep
            } else if client_only && strip_client_only {
                ModDisposition::Remove
            } else {
                ModDisposition::Keep
            };
            PlanMod {
                id,
                name: title_from_id(&lower_file_stem(&f.file_name)),
                version,
                loader: Some(loader_label.clone()),
                disposition,
                client_only,
                needs_review,
                auto_supplement: false,
                local_path: None,
                depends: Vec::new(),
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
                local_path: None,
            depends: Vec::new(),
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
            local_path: None,
            depends: Vec::new(),
        });
    }
    plan
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
            server_required: true,
            env_declared: true,
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
        let plan = build_plan(&parsed, true);
        // project_id 精确命中行 id；包内不存在的 "sodium" 被丢弃
        assert_eq!(plan[1].depends, vec!["geckolib".to_string()]);
    }
}
