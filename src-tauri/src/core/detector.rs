//! 转换方案生成：按证据阶梯裁决（jar 自证 > 平台构建反查 > 平台项目声明 >
//! 整合包 files[].env > 名称启发），名称关键字只在没有任何证据时兜底；
//! 输出 PlanMod[] 与计数。

use std::collections::HashSet;

use crate::core::env::{self, CodeMap, Evidence, EvidenceMap};
use crate::core::parser::{PackFile, ParsedPack};
use crate::models::{
    BytecodeHint, CfLink, EnvSource, LoaderKind, ModDisposition, PlanCounts, PlanMod, SideFlag,
};

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
    /// 没有任何端证据：判不出两端（归入剔除分组等人工确认，见 build_plan 的 `pending`）
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

/// mrpack 的 `files[].env` 整层有没有「区分度」。
/// 该字段是打包者抄来的第二手声明：Modrinth 网页/Prism 导出的包会带上模组作者的真声明，
/// 但第三方（尤其国内站）的打包工具普遍把整表刷成 `required/required`——那种表一个模组的
/// 支持度都没说，信它就等于全表保留。判据：只要有任一行出现 unsupported/optional，
/// 说明打包者真做了区分，整层可用；否则整层作废，让位给 jar 与平台证据。
pub fn mrpack_env_informative(files: &[PackFile]) -> bool {
    files.iter().any(|f| {
        [f.env_client, f.env_server]
            .into_iter()
            .flatten()
            .any(|s| matches!(s, SideFlag::Unsupported | SideFlag::Optional))
    })
}

/// 该行是否被（可信的）整合包声明答过——命令层据此决定要不要为它联网反查
pub fn env_declared(f: &PackFile, informative: bool) -> bool {
    informative && (f.env_client.is_some() || f.env_server.is_some())
}

/// 生成转换方案。
/// - `strip_client_only=false`：只给证据、不自动剔除（全部保留），供「关掉自动」的场景；
///   「判不出两端」的待人工确认分组同样归它管（手动模式不满屏标红）
/// - `ev`：包内条目 → 端证据（jar 自证 / Modrinth 反查 / 本地索引），键为条目路径。
///   它与 mrpack env 一起进同一个可信度排序（jar > 构建 > 项目 > mrpack > 名称），
///   mrpack 层仅在整包声明有区分度时参与；两者裁决不一致的行标 `env_conflict`
/// - `code`：包内条目 → jar 字节码结构事实。只改两件事：有服务端注册时按住名称关键字层，
///   以及给前端一句提示；不产生任何新的剔除
pub fn build_plan(
    parsed: &ParsedPack,
    strip_client_only: bool,
    ev: &EvidenceMap,
    code: &CodeMap,
) -> Vec<PlanMod> {
    let loader_label = parsed.manifest.loader.as_label().to_string();
    let informative = mrpack_env_informative(&parsed.mod_files);
    let mut plan: Vec<PlanMod> = parsed
        .mod_files
        .iter()
        .map(|f| {
            let (id, version) = split_mod_file(&f.file_name);
            let lower = f.file_name.to_lowercase();
            let needs_review = NEEDS_REVIEW_KEYWORDS.iter().any(|k| lower.contains(k));
            let facts = code.get(&f.path).copied().unwrap_or_default();

            // 证据阶梯：jar 自证 > 平台按构建反查 > 平台按项目 > 整合包声明 > 名称启发式
            let mut cands: Vec<Evidence> = Vec::new();
            if let Some(e) = ev.get(&f.path) {
                cands.push(*e);
            }
            if env_declared(f, informative) {
                cands.push(Evidence {
                    client: f.env_client,
                    server: f.env_server,
                    source: EnvSource::Mrpack,
                });
            }
            let chosen = cands
                .iter()
                .min_by_key(|e| env::rank(e.source))
                .copied();
            // 被丢掉的那层给出过不同结论 → 这行的判定值得让人亲眼看一下
            let env_conflict = chosen.is_some_and(|c| {
                cands.iter().any(|e| verdict(e.client, e.server) != verdict(c.client, c.server))
            });

            let name_hit = name_heuristic_hit(&f.file_name);
            // 名称表是这台机器上唯一会「凭空删模组」的层。jar 里确有服务端注册时不让它删：
            // 前者是解出来的结构事实，后者只是文件名像不像
            let vetoed = chosen.is_none() && name_hit && facts.server_code;
            // 「判不出来」= 证据阶梯全空且名称层也没给出结论。被 veto 的那行有 server_code
            // 这条结构事实（服务端确有代码会跑），属于有依据的保留，不算进来
            let undecidable = chosen.is_none() && !name_hit;
            let (client, server, source) = match chosen {
                Some(e) => (e.client, e.server, e.source),
                None => {
                    // 兜底：关键字命中 = 认定「客户端必需、服务端不支持」
                    if name_hit && !vetoed {
                        (
                            Some(SideFlag::Required),
                            Some(SideFlag::Unsupported),
                            EnvSource::NameHeuristic,
                        )
                    } else {
                        (None, None, EnvSource::Unknown)
                    }
                }
            };
            let mut strip = verdict(client, server) == Verdict::Strip;
            // 字节码否决扩展：jar 里确有服务端注册（common setup / 注册表 / 网络层——
            // 加载器自己的 API，不参与混淆，假阳性率实测很低）却被判剔除——声明与字节
            // 矛盾，宁保留+人工，不静默错删。实测案例（FarmingTales 包）：GeckoLib 是
            // 打包者塞进 overrides 的索引外文件（hash 反查落空），被 Modrinth 项目级
            // server_side=optional 判成客户端模组，服务端缺它直接起不来。
            // 覆盖 (必,可) 与 (·,不支持) 两种剔除；改判后强制待人工，让人看得见这台保险
            let bytecode_vetoed_strip = strip && chosen.is_some() && facts.server_code;
            if bytecode_vetoed_strip {
                strip = false;
            }
            let client_only = strip && strip_client_only;
            let bytecode_hint = if vetoed || bytecode_vetoed_strip {
                Some(BytecodeHint::ServerCode)
            } else if chosen.is_none() && !name_hit && facts.client_only_shape {
                Some(BytecodeHint::ClientOnlyShape)
            } else {
                None
            };

            // 判不出两端的行（上面每层都没答上、名称表也没猜中）：不悄悄留在服务端包里，
            // 归进剔除分组并强制人工确认——要它的服主自己勾回来，比默认塞进包里安全。
            // 例外：关键字表（Via* 那类）本身就是「有服务端价值、默认保留」的口径，不并进来；
            // 自动剔除关掉时整条不生效（那是手动模式，不该满屏标待确认）。
            let pending = undecidable && !needs_review && strip_client_only;
            // 「服务端轴没答上」= 这一行的处置缺的正是服务端那一条依据。分组照旧由裁决决定
            // （(必,无) 剔、(可,无) 留），但一律标出来让人过一眼——与前端 sideTagOf 的 review 同判据。
            // 被字节码按住名称层的那行不算：jar 里确有服务端注册，那是有依据的保留。
            let server_undecided = server.is_none() && !vetoed && strip_client_only;
            let review = server_undecided && !needs_review;
            PlanMod {
                id,
                name: title_from_id(&lower_file_stem(&f.file_name)),
                version,
                loader: Some(loader_label.clone()),
                disposition: if (client_only && !needs_review) || pending {
                    ModDisposition::Remove
                } else {
                    ModDisposition::Keep
                },
                client_only,
                // 字节码按住的剔除矛盾（自动剔除开着才标：手动模式本就没有自动剔除）
                needs_review: needs_review || review || (bytecode_vetoed_strip && strip_client_only),
                auto_supplement: false,
                size_bytes: f.size_bytes,
                // 只有「有 URL 可下且物理不在包内」才是真联网下载；
                // mrpack 正常条目全部内嵌 → 包内直取。
                // CF 那一档特殊：清单只给编号、**url 恒空**（直链带时效，构建期现取），
                // 但字节确实不在包里 ⇒ 也算联网下载，不能报成「包内直取」
                needs_download: !f.in_pack && (!f.url.is_empty() || f.cf.is_some()),
                local_path: None,
                pinned: None,
                depends: Vec::new(),
                src_path: Some(f.path.clone()),
                env_source: source,
                env_conflict,
                client_side: client,
                server_side: server,
                bytecode_hint,
                // CF 编号行的「两条取链路都拿不到字节」（自动分类那一轮探出来的）。
                // 挂在行上而不是另开一份清单：用户在界面上改判处置之后，闸门要按**当前这份方案**
                // 重算缺件数（被判为剔除的行本来就进不了服务端包，缺不缺件与它无关）
                cf_blocked: f.cf.as_ref().is_some_and(|r| r.link == CfLink::Unavailable),
                cf_required: f.cf.as_ref().is_some_and(|r| r.required),
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
            env_conflict: false,
            client_side: None,
            server_side: Some(SideFlag::Required),
            bytecode_hint: None,
            // 补齐行是我们自己加的、走 Modrinth 那条腿，与 CF 的取链许可无关
            cf_blocked: false,
            cf_required: false,
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
            cf: None,
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
            root_prefix: String::new(),
        };
        let plan = build_plan(&parsed, true, &EvidenceMap::new(), &CodeMap::new());
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
            root_prefix: String::new(),
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
            cf: None,
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
    fn uninformative_mrpack_env_yields_to_jar_self_proof() {
        use SideFlag::{Required, Unsupported};
        // 第三方打包工具把 files[].env 整表刷成 required/required：这层等于什么都没声明。
        // 此时它整层作废（不算证据、也不记冲突——没有结论的东西谈不上矛盾），让位给 jar 自证，
        // 否则纯客户端模组会永远留在保留区。
        let parsed = pack(vec![
            file("fancyhud-1.0.jar", (Some(Required), Some(Required))),
            file("some-lib-1.0.jar", (Some(Required), Some(Required))),
        ]);
        assert!(!mrpack_env_informative(&parsed.mod_files));

        let mut map = EvidenceMap::new();
        add(
            &mut map,
            "mods/fancyhud-1.0.jar",
            Required,
            Unsupported,
            EnvSource::JarMetadata,
        );
        let plan = build_plan(&parsed, true, &map, &CodeMap::new());
        assert_eq!(plan[0].disposition, ModDisposition::Remove);
        assert_eq!(plan[0].env_source, EnvSource::JarMetadata);
        assert!(!plan[0].env_conflict);
        // jar 也没答上的行：落回「判不出两端」→ 进剔除分组等人工确认
        assert_eq!(plan[1].disposition, ModDisposition::Remove);
        assert!(plan[1].needs_review);
        assert!(!plan[1].client_only, "没证据的行不该被说成客户端专属");
        assert_eq!(plan[1].env_source, EnvSource::Unknown);
    }

    #[test]
    fn informative_mrpack_env_wins_over_nothing_but_loses_to_jar() {
        use SideFlag::{Optional, Required, Unsupported};
        // 有一行给了 unsupported/optional → 打包者真做了区分 → 整层可用；
        // 但 jar 自证（加载器运行时强制执行）仍然压过它，结论相反的行标冲突。
        let parsed = pack(vec![
            file("sodium-0.5.13.jar", (Some(Required), Some(Required))),
            file("luckperms-1.0.jar", (Some(Required), Some(Optional))),
        ]);
        assert!(mrpack_env_informative(&parsed.mod_files));

        let mut map = EvidenceMap::new();
        add(
            &mut map,
            "mods/sodium-0.5.13.jar",
            Required,
            Unsupported,
            EnvSource::JarMetadata,
        );
        let plan = build_plan(&parsed, true, &map, &CodeMap::new());
        assert_eq!(plan[0].env_source, EnvSource::JarMetadata);
        assert_eq!(plan[0].disposition, ModDisposition::Remove);
        assert!(plan[0].env_conflict, "打包者说服务端必需，jar 说不支持");
        // 只有整合包声明、jar 没答上的行：按声明保留
        assert_eq!(plan[1].env_source, EnvSource::Mrpack);
        assert_eq!(plan[1].disposition, ModDisposition::Keep);
        assert!(!plan[1].env_conflict);
    }

    #[test]
    fn mrpack_declares_client_only_row_as_removed() {
        let parsed = pack(vec![file(
            "sodium-0.5.13.jar",
            (Some(SideFlag::Unsupported), Some(SideFlag::Required)),
        )]);
        let plan = build_plan(&parsed, true, &EvidenceMap::new(), &CodeMap::new());
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
        let plan = build_plan(&parsed, true, &map, &CodeMap::new());
        assert_eq!(plan[0].disposition, ModDisposition::Remove);
        assert_eq!(plan[0].env_source, EnvSource::ModrinthHash);
    }

    #[test]
    fn client_required_with_unknown_server_strips_but_asks_for_review() {
        use SideFlag::{Optional, Required};
        // 第三方 mrpack 常只写 client 一轴（服务端缺键）：剔除这一下没有服务端依据，
        // 所以行落进剔除分组，但同时标「需人工确认」并置顶——与前端 sideTagOf 的 review 同判据
        let mut map = EvidenceMap::new();
        map.insert(
            "mods/half-declared-1.0.jar".to_string(),
            Evidence {
                client: Some(Required),
                server: None,
                source: EnvSource::ModrinthProject,
            },
        );
        map.insert(
            "mods/half-declared-lib-1.0.jar".to_string(),
            Evidence {
                client: Some(Optional),
                server: None,
                source: EnvSource::ModrinthProject,
            },
        );
        let parsed = pack(vec![
            file("half-declared-1.0.jar", (None, None)),
            file("half-declared-lib-1.0.jar", (None, None)),
        ]);
        let plan = build_plan(&parsed, true, &map, &CodeMap::new());
        assert_eq!(plan[0].disposition, ModDisposition::Remove);
        assert!(plan[0].needs_review, "服务端轴没答上 → 交给人");
        assert!(plan[0].client_only, "客户端那一轴确实答上了：必需");
        // 分组照旧由裁决决定：客户端只是可选 → 留在包里，但一样要人过一眼
        assert_eq!(plan[1].disposition, ModDisposition::Keep);
        assert!(plan[1].needs_review);
        // 手动模式（自动剔除关掉）不该满屏标待确认
        let manual = build_plan(&parsed, false, &map, &CodeMap::new());
        assert_eq!(manual[0].disposition, ModDisposition::Keep);
        assert!(!manual[0].needs_review);
        assert!(!manual[1].needs_review);
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
        let plan = build_plan(&parsed, true, &map, &CodeMap::new());
        assert_eq!(plan[0].disposition, ModDisposition::Keep);
        assert_eq!(plan[1].disposition, ModDisposition::Keep);
    }

    #[test]
    fn undecidable_row_is_flagged_for_review() {
        // 「没证据」不再等于「静默保留」：标出来让人看一眼，分组落在剔除侧
        let parsed = pack(vec![file("some-obscure-lib-1.0.jar", (None, None))]);
        let plan = build_plan(&parsed, true, &EvidenceMap::new(), &CodeMap::new());
        assert_eq!(plan[0].disposition, ModDisposition::Remove);
        assert!(plan[0].needs_review);
        assert_eq!(plan[0].env_source, EnvSource::Unknown);
        assert_eq!(plan[0].client_side, None);
        assert_eq!(plan[0].server_side, None);
    }

    #[test]
    fn name_heuristic_is_last_resort_and_labels_its_source() {
        let parsed = pack(vec![file("continuity-3.0.jar", (None, None))]);
        let plan = build_plan(&parsed, true, &EvidenceMap::new(), &CodeMap::new());
        assert_eq!(plan[0].disposition, ModDisposition::Remove);
        assert_eq!(plan[0].env_source, EnvSource::NameHeuristic);
    }

    /* ---------------- 字节码结构事实（只准少删、不准多删） ---------------- */

    use crate::core::env::CodeFacts;

    fn facts(path: &str, server_code: bool, client_only_shape: bool) -> CodeMap {
        let mut m = CodeMap::new();
        m.insert(
            path.to_string(),
            CodeFacts {
                server_code,
                client_only_shape,
            },
        );
        m
    }

    #[test]
    fn server_code_fact_vetoes_the_name_heuristic() {
        // 名称表是这台机器上唯一凭空删模组的层：jar 里确有服务端注册时按住它
        let parsed = pack(vec![file("continuity-3.0.jar", (None, None))]);
        let plan = build_plan(
            &parsed,
            true,
            &EvidenceMap::new(),
            &facts("mods/continuity-3.0.jar", true, false),
        );
        assert_eq!(plan[0].disposition, ModDisposition::Keep);
        // 有服务端结构事实 = 有依据的保留，不该跟「判不出来」混进待确认分组
        assert!(!plan[0].needs_review);
        // 被按住的是「猜的」那层，所以来源仍是无证据，不能冒充有证据
        assert_eq!(plan[0].env_source, EnvSource::Unknown);
        assert_eq!(plan[0].bytecode_hint, Some(BytecodeHint::ServerCode));
    }

    #[test]
    fn client_only_shape_lands_in_review_not_as_a_client_only_strip() {
        // 形状提示 + 判不出两端：行进的是「待人工确认」，不是「客户端专属」——
        // 前者可以被勾回，后者对外宣称的是结论
        let parsed = pack(vec![file("some-obscure-lib-1.0.jar", (None, None))]);
        let plan = build_plan(
            &parsed,
            true,
            &EvidenceMap::new(),
            &facts("mods/some-obscure-lib-1.0.jar", false, true),
        );
        assert_eq!(plan[0].disposition, ModDisposition::Remove);
        assert!(plan[0].needs_review);
        assert!(!plan[0].client_only);
        assert_eq!(plan[0].bytecode_hint, Some(BytecodeHint::ClientOnlyShape));
    }

    #[test]
    fn undecidable_rows_park_in_remove_for_review_not_silently_kept() {
        // 判不出两端的行不悄悄留在服务端包里（用户 2026-09-21 拍板）：进剔除分组 + 标待确认
        let parsed = pack(vec![
            file("some-obscure-lib-1.0.jar", (None, None)),
            file("sodium-0.5.13.jar", (None, None)),
        ]);
        let plan = build_plan(&parsed, true, &EvidenceMap::new(), &CodeMap::new());
        assert_eq!(plan[0].disposition, ModDisposition::Remove);
        assert!(plan[0].needs_review);
        assert_eq!(plan[0].env_source, EnvSource::Unknown);
        // 名称表猜出来的那行照旧按名称层走，不算「判不出」（也别改标待确认，会满屏噪音）
        assert_eq!(plan[1].disposition, ModDisposition::Remove);
        assert!(!plan[1].needs_review);
        assert_eq!(plan[1].env_source, EnvSource::NameHeuristic);
    }

    #[test]
    fn review_keyword_rows_stay_kept_even_without_evidence() {
        // Via* 那类关键字表的口径是「有服务端价值、默认保留」，不被新规则一并卷进剔除分组
        let parsed = pack(vec![file("viafabricplus-3.4.11.jar", (None, None))]);
        let plan = build_plan(&parsed, true, &EvidenceMap::new(), &CodeMap::new());
        assert_eq!(plan[0].disposition, ModDisposition::Keep);
        assert!(plan[0].needs_review);
    }

    #[test]
    fn auto_off_leaves_undecidable_rows_in_keep_without_review_noise() {
        // 关掉自动剔除 = 手动模式：不逐行标待确认，分组也不动
        let parsed = pack(vec![file("some-obscure-lib-1.0.jar", (None, None))]);
        let plan = build_plan(&parsed, false, &EvidenceMap::new(), &CodeMap::new());
        assert_eq!(plan[0].disposition, ModDisposition::Keep);
        assert!(!plan[0].needs_review);
    }

    #[test]
    fn server_code_vetoes_a_strip_verdict_even_from_evidence() {
        //（GeckoLib 案例，2026-10 实测）jar 字节里确有服务端注册，声明却说服务端不要——
        // 平台声明会错（作者把库模组的 server_side 标成 optional，服务端缺它起不来），
        // 字节不会。宁保留+人工，不静默剔出一个起不来的服务端。
        // 旧契约「证据层答上后字节码闭嘴」由此推翻：假阳性的代价只是多保留一个待人工，
        // 假阴性的代价是服务端缺件开不起
        let parsed = pack(vec![file("geckolib-forge-1.20.1-4.8.4.jar", (None, None))]);

        // (必,可)：Modrinth 项目级声明把 GeckoLib 判成客户端模组的那一档
        let mut project = EvidenceMap::new();
        add(
            &mut project,
            "mods/geckolib-forge-1.20.1-4.8.4.jar",
            SideFlag::Required,
            SideFlag::Optional,
            EnvSource::ModrinthProject,
        );
        let plan = build_plan(
            &parsed,
            true,
            &project,
            &facts("mods/geckolib-forge-1.20.1-4.8.4.jar", true, false),
        );
        assert_eq!(plan[0].disposition, ModDisposition::Keep);
        assert!(plan[0].needs_review, "矛盾必须亮给人看");
        assert_eq!(plan[0].bytecode_hint, Some(BytecodeHint::ServerCode));

        // (必,不支持)：矛盾更狠的一档，同样按住
        let mut meta = EvidenceMap::new();
        add(
            &mut meta,
            "mods/geckolib-forge-1.20.1-4.8.4.jar",
            SideFlag::Required,
            SideFlag::Unsupported,
            EnvSource::JarMetadata,
        );
        let plan = build_plan(
            &parsed,
            true,
            &meta,
            &facts("mods/geckolib-forge-1.20.1-4.8.4.jar", true, false),
        );
        assert_eq!(plan[0].disposition, ModDisposition::Keep);
        assert!(plan[0].needs_review);
    }

    #[test]
    fn strip_switch_off_keeps_everything_but_keeps_the_evidence() {
        let parsed = pack(vec![file(
            "sodium-0.5.13.jar",
            (Some(SideFlag::Unsupported), Some(SideFlag::Required)),
        )]);
        let plan = build_plan(&parsed, false, &EvidenceMap::new(), &CodeMap::new());
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
        let plan = build_plan(&parsed, true, &EvidenceMap::new(), &CodeMap::new());
        assert_eq!(plan[0].disposition, ModDisposition::Keep);
        assert!(plan[0].needs_review, "被保留行硬依赖，应强制保留并标待确认");
    }
}
