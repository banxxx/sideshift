//! 证据结构与可信度阶梯（裁决规则在 detector，这里只定义「一条证据长什么样、谁说了算」）。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::core::downloader::ModrinthEnv;
use crate::models::{EnvSource, SideFlag};

/// 一条两侧支持度证据及其出处
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Evidence {
    pub client: Option<SideFlag>,
    pub server: Option<SideFlag>,
    pub source: EnvSource,
}

/// 包内条目路径 → 证据（第 2~4 层的汇总；第 1 层由 parser 挂在 PackFile 上）
pub type EvidenceMap = HashMap<String, Evidence>;

/// 来源可信度排序（越小越可信）。
/// **mrpack 的 `files[].env` 排在 jar 自证与平台反查之后**：它是打包者抄下来的第二手声明，
/// 第三方工具还普遍把整表刷成 required/required（一个支持度都没说）。jar 内的
/// `environment` 是加载器运行时强制执行的，平台侧声明则由模组作者在自己项目上维护。
/// 镜像档（`MirrorProject`）内容就是 Modrinth 的项目级声明，但它是第三方快照、可能滞后，
/// 所以永远排在官方项目层之下：官方答上过的那一行，镜像结论压不动它。
/// 百科档（`Mcmod`）是社区编辑按模组自述整理的词条字段：它专门回答「这一端要不要装」这一件事，
/// 比打包者顺手抄的整表声明更有针对性，但同样是第二手、且跟着编辑的校对走 ⇒ 排在所有平台层之下、
/// mrpack 声明之上。它只在平台各腿全答不上时才有机会出场（见 `index::resolve_via_mcmod`）。
pub fn rank(s: EnvSource) -> u8 {
    match s {
        EnvSource::JarMetadata => 0,
        EnvSource::ModrinthHash => 1,
        EnvSource::ModrinthProject => 2,
        EnvSource::MirrorProject => 3,
        EnvSource::Mcmod => 4,
        EnvSource::Mrpack => 5,
        EnvSource::NameHeuristic => 6,
        EnvSource::Unknown => 7,
    }
}

/// 写入证据：仅在来源更可信（或同级＝更新）时覆盖
pub fn put(map: &mut EvidenceMap, path: &str, ev: Evidence) {
    if map
        .get(path)
        .is_some_and(|cur| rank(cur.source) < rank(ev.source))
    {
        return;
    }
    map.insert(path.to_string(), ev);
}

/// Modrinth 端信息 → 证据（两侧映射见 `ModrinthEnv::sides`，在线添加的端标签共用）
pub fn evidence_from_modrinth(m: &ModrinthEnv, source: EnvSource) -> Option<Evidence> {
    m.sides().map(|(client, server)| Evidence {
        client: Some(client),
        server: Some(server),
        source,
    })
}
