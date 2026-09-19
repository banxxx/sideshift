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
            }
        })
        .collect();

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
}
