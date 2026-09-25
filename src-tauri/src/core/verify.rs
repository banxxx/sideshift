//! 构建后静态自检：打包完成、staging 还没回收之前，对着**真实落盘的产物**核对七件事。
//!
//! 全程离线、零子进程。这里回答的是「这份 zip 少没少东西、有没有半截 jar」，
//! 不是「服务端能不能起来」——后者要 Java、要同意 EULA，未开本机安装那一档的 Forge 还要首次
//! 联网自装服务端，失败原因九成与本包无关，把它做成构建开关只会让用户误信「跑起来了 = 包没问题」。
//! 所以结论口径统一是**对账**，报告页也不得写成「校验通过 = 可开服」。

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::models::{
    CheckResult, CheckStatus, ConversionOptions, LoaderKind, ModDisposition, PlanMod,
};

/// 单项最多列几个对象名：报告页是一张卡，缺 200 个文件时不能真列 200 行
const MAX_ITEMS: usize = 8;

pub struct Input<'a> {
    /// 打包用的暂存目录（此刻内容与 zip 一致，zip 写完才回收）
    pub staging: &'a Path,
    pub options: &'a ConversionOptions,
    pub loader: LoaderKind,
    pub plan: &'a [PlanMod],
    /// 启动脚本指向的 jar 名
    pub start_jar: Option<&'a str>,
    /// 新式已装布局的两份参数文件（包内相对路径）。非空时"启动指向"对账的就是它们，
    /// 因为那一态没有单一 jar 可指（`start_jar` 会是 None）
    pub args_files: &'a [String],
    /// 本次把本机装好的 loader 树并进了 staging：启动不再指向某一枚 jar，对账维度换成依赖树
    pub installed: bool,
    /// builder 报的本次包根生成文件
    pub generated: &'a [String],
    /// 取件前登记的 mods/ 应到文件名：下载静默少一条，只有对账才看得出来
    pub expected_mod_files: &'a [String],
    /// 保留目录 → 计划带入的文件数
    pub expected_keep_dirs: &'a HashMap<String, usize>,
}

/// 七项自检（未勾选保留目录时不出最后一项：空对空的「通过」是噪音）
pub fn run(input: &Input) -> Vec<CheckResult> {
    let mut out = vec![
        fetch_check(input),
        jar_check(input),
        deps_check(input),
        start_check(input),
        loader_check(input),
        root_check(input),
    ];
    if !input.options.keep_dirs.is_empty() {
        out.push(keep_check(input));
    }
    out
}

fn check(id: &str, label: &str, status: CheckStatus, detail: String, items: Vec<String>) -> CheckResult {
    let mut items = items;
    items.truncate(MAX_ITEMS);
    CheckResult { id: id.to_string(), label: label.to_string(), status, detail, items }
}

/// 1 · 取件完整：计划落位的模组文件是否一个不少
fn fetch_check(input: &Input) -> CheckResult {
    let mods = input.staging.join("mods");
    let present: HashSet<String> = files_under(&mods)
        .iter()
        .filter_map(|p| p.file_name().map(|s| s.to_string_lossy().to_lowercase()))
        .collect();
    let expected = input.expected_mod_files.len();
    let missing: Vec<String> = input
        .expected_mod_files
        .iter()
        .filter(|n| !present.contains(&n.to_lowercase()))
        .cloned()
        .collect();
    let status = if missing.is_empty() { CheckStatus::Pass } else { CheckStatus::Fail };
    let detail = if expected == 0 {
        "本包无模组文件需落位".to_string()
    } else if missing.is_empty() {
        format!("模组 {expected} 个全部落位")
    } else {
        format!("模组应到 {expected} 个 · 缺 {} 个", missing.len())
    };
    check("files", "取件完整", status, detail, missing)
}

/// 2 · jar 可用：非 0 字节且真是 zip 容器。镜像/CDN 给的半截文件或 HTML 错误页
/// 在取件阶段未必拦得住（无 sha1 的坐标不校验），到这儿按魔数兜一层
fn jar_check(input: &Input) -> CheckResult {
    let mut jars = files_under(&input.staging.join("mods"));
    jars.extend(
        files_under(input.staging)
            .into_iter()
            .filter(|p| p.parent() == Some(input.staging)),
    );
    let jars: Vec<PathBuf> = jars
        .into_iter()
        .filter(|p| matches!(ext_of(p).as_str(), "jar" | "zip"))
        .collect();
    let bad: Vec<String> = jars
        .iter()
        .filter(|p| !readable_container(p))
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .collect();
    let status = if bad.is_empty() { CheckStatus::Pass } else { CheckStatus::Fail };
    let detail = if jars.is_empty() {
        "无 jar 需校验".to_string()
    } else if bad.is_empty() {
        format!("{} 个 jar 容器可读", jars.len())
    } else {
        format!("{} 个 jar 不完整或非压缩包", bad.len())
    };
    check("jars", "jar 可用", status, detail, bad)
}

/// 3 · 依赖闭合：保留行的 depends 有没有指向被剔除的行。
/// detector 端有依赖图保护，这一项是那道保护的回归网——真漏了就是开服即崩的漏法
fn deps_check(input: &Input) -> CheckResult {
    let by_id: HashMap<&str, &PlanMod> =
        input.plan.iter().map(|m| (m.id.as_str(), m)).collect();
    let kept = |d: ModDisposition| d != ModDisposition::Remove;
    let mut broken: Vec<String> = Vec::new();
    let mut refs = 0usize;
    for row in input.plan.iter().filter(|m| kept(m.disposition)) {
        for dep in &row.depends {
            // 依赖 id 不在方案里（外部新增没解析出依赖）不算问题：没证据就不指控
            let Some(target) = by_id.get(dep.as_str()) else { continue };
            refs += 1;
            if !kept(target.disposition) {
                broken.push(format!("{} → 已剔除的 {}", row.name, target.name));
            }
        }
    }
    let status = if broken.is_empty() { CheckStatus::Pass } else { CheckStatus::Fail };
    let detail = if refs == 0 {
        "方案内无交叉依赖".to_string()
    } else if broken.is_empty() {
        format!("{refs} 条依赖引用全部指向包内")
    } else {
        format!("{} 条依赖指向已剔除的模组", broken.len())
    };
    check("deps", "依赖闭合", status, detail, broken)
}

/// 4 · 启动指向：start 脚本引用的那个目标真的在包里。
/// 三态各查各的（实测三种布局）：新式已装查 `libraries/` 下那两份参数文件，老 Forge 与未装态查那一枚 jar。
/// builder 已经认过一次形状，这里仍对着 staging 复查明落盘没有——并树、写脚本、打包是三步，
/// 中间任何一步漏了文件，报告不能跟着 builder 的结论说"就位"。
fn start_check(input: &Input) -> CheckResult {
    let targets: Vec<&str> = if !input.args_files.is_empty() {
        input.args_files.iter().map(String::as_str).collect()
    } else if let Some(jar) = input.start_jar {
        vec![jar]
    } else {
        return check(
            "start",
            "启动指向",
            CheckStatus::Warn,
            "未确定启动目标，需自行指定".to_string(),
            Vec::new(),
        );
    };
    let missing: Vec<String> = targets
        .iter()
        .filter(|t| !input.staging.join(t).is_file())
        .map(|t| t.to_string())
        .collect();
    if !missing.is_empty() {
        return check(
            "start",
            "启动指向",
            CheckStatus::Fail,
            format!("启动脚本指向的 {} 个文件不在包里", missing.len()),
            missing,
        );
    }
    let head = if targets.len() > 1 {
        "两份参数文件都在包内".to_string()
    } else {
        format!("{} 就位", targets[0])
    };
    if !input.options.generate_scripts {
        return check(
            "start",
            "启动指向",
            CheckStatus::Warn,
            format!("{head}；本次未生成启动脚本，需自行按包内文件启动"),
            Vec::new(),
        );
    }
    let tail = match (input.installed, input.loader) {
        // 本机装好的那一态没有联网这一步
        (true, _) => "加载器与依赖已打进包，解压后可直接跑",
        // Fabric 与"没本机安装"的 Forge 都要在首启现装：这句必须说，
        // 否则用户以为「自检通过 = 双击就能开服」而把联网等待当成卡死
        (false, LoaderKind::Fabric) => "首次运行会联网装出 loader（需本机 Java 与网络）",
        (false, LoaderKind::Forge | LoaderKind::NeoForge) => {
            "首次运行会自动安装服务端（需本机 Java 与网络）"
        }
    };
    check("start", "启动指向", CheckStatus::Pass, format!("{head} · {tail}"), Vec::new())
}

/// 各家 loader 本体的文件名前缀（实测三种布局都认这一条，比猜 Maven 目录稳）：
/// 新式 = `libraries/net/minecraftforge/forge/<ver>/forge-<ver>-universal.jar`，
/// 老 Forge = 包根 `forge-1.16.5-36.2.39.jar`，NeoForge 同 Forge 的新式布局换了组名
fn loader_prefix(loader: LoaderKind) -> &'static str {
    match loader {
        LoaderKind::Fabric => "fabric-loader-",
        LoaderKind::Forge => "forge-",
        LoaderKind::NeoForge => "neoforge-",
    }
}

/// 包内（含 `libraries/` 整棵依赖树）那枚 loader 本体
fn loader_jar(input: &Input) -> Option<String> {
    let prefix = loader_prefix(input.loader);
    let mut names: Vec<PathBuf> = files_under(input.staging)
        .into_iter()
        .filter(|p| p.parent() == Some(input.staging))
        .collect();
    names.extend(files_under(&input.staging.join("libraries")));
    names
        .iter()
        .filter_map(|p| p.file_name().map(|s| s.to_string_lossy().to_ascii_lowercase()))
        .find(|n| n.starts_with(prefix) && n.ends_with(".jar") && !n.contains("installer"))
        // 名字带 .jar 读着最顺，所以内部留后缀、出去时再摘（detail 与 items 都写成 `xxx.jar`）
        .map(|n| n.trim_end_matches(".jar").to_string())
}

/// 5 · Loader 就位：这一包的加载器到底是「已经装好在包里」还是「首启才装」。
/// 与第 4 项分开是因为它们查的是两件事——脚本指得对不对，和被指的那个东西齐不齐。
fn loader_check(input: &Input) -> CheckResult {
    let found = loader_jar(input);
    if input.installed {
        return match found {
            Some(name) => check(
                "loader",
                "Loader 就位",
                CheckStatus::Pass,
                format!("本机装好的加载器已打进包（{name}.jar）· 目标机无需联网安装"),
                Vec::new(),
            ),
            None => check(
                "loader",
                "Loader 就位",
                CheckStatus::Fail,
                "本机装好了加载器，但依赖树里没有它的本体（并树这一步没跑成）".to_string(),
                vec![format!("{}*.jar", loader_prefix(input.loader))],
            ),
        };
    }
    // 没开本机安装：包里的加载器还是「待安装」状态，那一枚 installer / Fabric 服务端 jar 就是全部依据。
    // 齐了也只是"能装"，所以走提示档而不是通过档
    match (input.start_jar, found) {
        (_, Some(name)) => check(
            "loader",
            "Loader 就位",
            CheckStatus::Pass,
            format!("包内已有加载器本体（{name}.jar）"),
            Vec::new(),
        ),
        (Some(jar), None) => check(
            "loader",
            "Loader 就位",
            CheckStatus::Warn,
            format!("只有 {jar}·首次运行才联网装出加载器"),
            Vec::new(),
        ),
        (None, None) => check(
            "loader",
            "Loader 就位",
            CheckStatus::Fail,
            "包内既没有加载器本体也没有安装器".to_string(),
            Vec::new(),
        ),
    }
}

/// 6 · 包根文件：builder 报的生成件真的落盘了，且必需件一个不少
fn root_check(input: &Input) -> CheckResult {
    let missing: Vec<String> = input
        .generated
        .iter()
        .filter(|n| !nonempty(&input.staging.join(n)))
        .cloned()
        .collect();
    // eula.txt 与 server.properties 恒生成（builder 无条件写），start.* 才随开关走。
    // 缺这些不致命（服务端首启会自建），所以只进提示档，不占「未通过」
    let mut soft: Vec<&str> = vec!["eula.txt", "server.properties"];
    if input.options.generate_scripts {
        soft.extend(["start.bat", "start.sh"]);
    }
    let soft: Vec<String> = soft
        .iter()
        .filter(|n| !input.staging.join(**n).is_file())
        .map(|n| n.to_string())
        .collect();
    if !missing.is_empty() {
        return check(
            "root",
            "包根文件",
            CheckStatus::Fail,
            format!("{} 个已登记的包根文件不在包里", missing.len()),
            missing,
        );
    }
    if !soft.is_empty() {
        return check(
            "root",
            "包根文件",
            CheckStatus::Warn,
            format!("缺 {} 个常用包根文件", soft.len()),
            soft,
        );
    }
    check(
        "root",
        "包根文件",
        CheckStatus::Pass,
        format!("包根 {} 个文件全部就位", input.generated.len()),
        Vec::new(),
    )
}

/// 7 · 保留目录：勾选的目录按取件阶段的账本再数一遍实际落位数
fn keep_check(input: &Input) -> CheckResult {
    let mut short: Vec<String> = Vec::new();
    let mut total = 0usize;
    // 按勾选顺序走（不是 HashMap 迭代序）：报告要能稳定复现
    for dir in &input.options.keep_dirs {
        let Some(expect) = input.expected_keep_dirs.get(dir) else { continue };
        total += expect;
        let have = files_under(&input.staging.join(dir)).len();
        if have < *expect {
            short.push(format!("{dir}（应 {expect} · 实 {have}）"));
        }
    }
    let status = if short.is_empty() { CheckStatus::Pass } else { CheckStatus::Warn };
    let detail = format!(
        "{} 个目录 · {} 个文件已带入",
        input.expected_keep_dirs.len(),
        total
    );
    check("keep", "保留目录", status, detail, short)
}

/* ---------------- 文件系统小工具 ---------------- */

fn ext_of(p: &Path) -> String {
    p.extension()
        .map(|s| s.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

fn nonempty(p: &Path) -> bool {
    fs::metadata(p).is_ok_and(|m| m.is_file() && m.len() > 0)
}

/// 目录内文件递归清单；目录不存在返回空（缺目录由调用方定级，不在这里炸）
fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(cur) = stack.pop() {
        let Ok(entries) = fs::read_dir(&cur) else { continue };
        for e in entries.flatten() {
            let path = e.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                out.push(path);
            }
        }
    }
    out
}

/// zip 魔数 + 非空：半截下载和错误页都过不了这道
fn readable_container(p: &Path) -> bool {
    let Ok(mut f) = fs::File::open(p) else { return false };
    let mut head = [0u8; 4];
    f.read_exact(&mut head).is_ok() && head == *b"PK\x03\x04"
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn row(id: &str, name: &str, disposition: ModDisposition, depends: &[&str]) -> PlanMod {
        PlanMod {
            id: id.into(),
            name: name.into(),
            version: "1.0".into(),
            loader: None,
            disposition,
            client_only: false,
            needs_review: false,
            auto_supplement: false,
            size_bytes: 0,
            needs_download: false,
            local_path: None,
            pinned: None,
            depends: depends.iter().map(|s| s.to_string()).collect(),
            src_path: None,
            env_source: Default::default(),
            env_conflict: false,
            client_side: None,
            server_side: None,
            bytecode_hint: None,
        }
    }

    /// 造一个暂存目录：mods 放若干 jar（空字节切片表示坏件），包根放指定名字的散件
    fn temp_staging(jars: &[(&str, &[u8])], root_files: &[(&str, &[u8])]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sideshift-verify-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(dir.join("mods")).unwrap();
        let good: &[u8] = b"PK\x03\x04 fake jar body";
        for (name, body) in jars {
            let path = if name.contains('/') {
                let full = dir.join(name);
                fs::create_dir_all(full.parent().unwrap()).unwrap();
                full
            } else {
                dir.join("mods").join(name)
            };
            let mut f = fs::File::create(&path).unwrap();
            f.write_all(if body.is_empty() { good } else { body }).unwrap();
        }
        for (name, body) in root_files {
            fs::write(dir.join(name), body).unwrap();
        }
        dir
    }

    /// 跑这几项时不关心的字段给空表即可
    fn report(dir: &Path, plan: &[PlanMod], expected: &[&str], generated: &[&str]) -> Vec<CheckResult> {
        let expected: Vec<String> = expected.iter().map(|s| s.to_string()).collect();
        let generated: Vec<String> = generated.iter().map(|s| s.to_string()).collect();
        run(&Input {
            staging: dir,
            options: &ConversionOptions::default(),
            loader: LoaderKind::Fabric,
            plan,
            start_jar: Some("fabric-server.jar"),
            args_files: &[],
            installed: false,
            generated: &generated,
            expected_mod_files: &expected,
            expected_keep_dirs: &HashMap::new(),
        })
    }

    fn status_of(checks: &[CheckResult], id: &str) -> CheckStatus {
        checks.iter().find(|c| c.id == id).unwrap().status
    }

    #[test]
    fn clean_pack_passes_every_check() {
        let dir = temp_staging(
            &[("a.jar", b""), ("b.jar", b"")],
            &[
                ("fabric-server.jar", b"PK\x03\x04".as_slice()),
                ("start.bat", b"@echo off".as_slice()),
                ("start.sh", b"#!/bin/sh".as_slice()),
                ("eula.txt", b"eula=true".as_slice()),
                ("server.properties", b"port=25565".as_slice()),
            ],
        );
        let plan = vec![row("a", "A", ModDisposition::Keep, &["b"])];
        let checks = report(&dir, &plan, &["a.jar", "b.jar"], &["start.bat", "eula.txt"]);
        assert!(
            checks
                .iter()
                .all(|c| c.status == CheckStatus::Pass || c.id == "loader"),
            "{:?}",
            checks.iter().map(|c| (&c.id, &c.detail)).collect::<Vec<_>>()
        );
        // 这一份是「没本机安装」的 Fabric 包：包里只有官方服务端 jar，加载器要首启现装。
        // 「Loader 就位」因此恒在提示档——它不是产物缺件，但也不该被读成"上传即跑"
        assert_eq!(status_of(&checks, "loader"), CheckStatus::Warn);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_mod_file_and_truncated_jar_both_fail() {
        let dir = temp_staging(
            &[("a.jar", b""), ("broken.jar", b"not a zip")],
            &[("fabric-server.jar", b"PK\x03\x04".as_slice())],
        );
        let checks = report(&dir, &[], &["a.jar", "gone.jar"], &[]);
        assert_eq!(status_of(&checks, "files"), CheckStatus::Fail);
        let files = checks.iter().find(|c| c.id == "files").unwrap();
        assert_eq!(files.items, vec!["gone.jar".to_string()]);
        assert_eq!(status_of(&checks, "jars"), CheckStatus::Fail);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn dependency_pointing_at_a_removed_row_fails() {
        let dir = temp_staging(&[("core.jar", b"")], &[("fabric-server.jar", b"PK\x03\x04".as_slice())]);
        let plan = vec![
            row("core", "Core Lib", ModDisposition::Keep, &["opti"]),
            row("opti", "OptiFine", ModDisposition::Remove, &[]),
        ];
        let checks = report(&dir, &plan, &["core.jar"], &[]);
        assert_eq!(status_of(&checks, "deps"), CheckStatus::Fail);
        let deps = checks.iter().find(|c| c.id == "deps").unwrap();
        assert!(deps.items[0].contains("OptiFine"), "{}", deps.items[0]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn keep_dir_shortfall_warns_and_absent_dir_is_skipped() {
        let dir = temp_staging(
            &[("mods/keep.jar", b""), ("config/a.toml", b"x".as_slice())],
            &[("fabric-server.jar", b"PK\x03\x04".as_slice())],
        );
        let mut expected = HashMap::new();
        // config 计划 3 个只落 1 个 → 提示；kubejs 计划 0 个 → 不进账本，也不报错
        expected.insert("config".to_string(), 3usize);
        let opts = ConversionOptions {
            keep_dirs: vec!["config".into(), "kubejs".into()],
            ..Default::default()
        };
        let generated = vec!["start.bat".to_string()];
        let checks = run(&Input {
            staging: &dir,
            options: &opts,
            loader: LoaderKind::Fabric,
            plan: &[],
            start_jar: Some("fabric-server.jar"),
            args_files: &[],
            installed: false,
            generated: &generated,
            expected_mod_files: &["keep.jar".to_string()],
            expected_keep_dirs: &expected,
        });
        assert_eq!(status_of(&checks, "keep"), CheckStatus::Warn);
        let keep = checks.iter().find(|c| c.id == "keep").unwrap();
        assert!(keep.items[0].starts_with("config"), "{}", keep.items[0]);
        assert_eq!(status_of(&checks, "files"), CheckStatus::Pass);
        let _ = fs::remove_dir_all(&dir);
    }

    /// 本机装好的新式布局：第四项查脚本引用的两份参数文件，第五项查依赖树里的加载器本体。
    /// 两条判据分开才有意义——树并进来了但本体没在里面，起跳照样是死的
    #[test]
    fn installed_pack_checks_args_files_and_loader_jar() {
        let args = "libraries/net/minecraftforge/forge/1.20.1-47.4.10";
        let full = temp_staging(
            &[
                ("mods/a.jar", b""),
                (&format!("{args}/win_args.txt"), b"-cp".as_slice()),
                (&format!("{args}/unix_args.txt"), b"-cp".as_slice()),
                (
                    &format!("{args}/forge-1.20.1-47.4.10-universal.jar"),
                    b"PK\x03\x04".as_slice(),
                ),
            ],
            &[("start.bat", b"@echo off".as_slice())],
        );
        // 参数文件在、加载器本体不在：树看着茂，其实是空的
        let no_loader = temp_staging(
            &[
                ("mods/a.jar", b""),
                (&format!("{args}/win_args.txt"), b"-cp".as_slice()),
                (&format!("{args}/unix_args.txt"), b"-cp".as_slice()),
            ],
            &[("start.bat", b"@echo off".as_slice())],
        );
        // 本体在、脚本指的第二份参数文件没打进包：起跳目标缺件
        let no_args = temp_staging(
            &[
                ("mods/a.jar", b""),
                (&format!("{args}/win_args.txt"), b"-cp".as_slice()),
                (
                    &format!("{args}/forge-1.20.1-47.4.10-universal.jar"),
                    b"PK\x03\x04".as_slice(),
                ),
            ],
            &[("start.bat", b"@echo off".as_slice())],
        );
        let args_files: Vec<String> = vec![format!("{args}/win_args.txt"), format!("{args}/unix_args.txt")];
        let of = |dir: &Path, id: &str, args: &[String]| -> CheckResult {
            let generated = vec!["start.bat".to_string()];
            let expected = vec!["a.jar".to_string()];
            run(&Input {
                staging: dir,
                options: &ConversionOptions::default(),
                loader: LoaderKind::Forge,
                plan: &[],
                start_jar: None,
                args_files: args,
                installed: true,
                generated: &generated,
                expected_mod_files: &expected,
                expected_keep_dirs: &HashMap::new(),
            })
            .into_iter()
            .find(|c| c.id == id)
            .unwrap()
        };

        assert_eq!(of(&full, "start", &args_files).status, CheckStatus::Pass);
        let loader = of(&full, "loader", &args_files);
        assert_eq!(loader.status, CheckStatus::Pass, "{}", loader.detail);
        assert!(loader.detail.contains("无需联网"), "{}", loader.detail);
        // 并树没跑成的那一种要红：本机装好了却没打进包，比缺一枚 jar 更该拦
        assert_eq!(of(&no_loader, "loader", &args_files).status, CheckStatus::Fail);
        // 脚本声明的两份参数文件少一份就是死的：报的得是缺的那一份，不是"树空"
        let miss = of(&no_args, "start", &args_files);
        assert_eq!(miss.status, CheckStatus::Fail, "{}", miss.detail);
        assert!(miss.items[0].ends_with("unix_args.txt"), "{:?}", miss.items);
        for dir in [&full, &no_loader, &no_args] {
            let _ = fs::remove_dir_all(dir);
        }
    }

    /// 老 Forge（实测 1.16.5）：装完没有参数文件，包根那枚 universal jar 就是加载器本体。
    /// 两项都要认这同一枚文件，但判据不同——脚本指得对不对 / 本体在不在
    #[test]
    fn installed_old_forge_reconciles_the_universal_jar() {
        let dir = temp_staging(
            &[("mods/a.jar", b"")],
            &[
                ("forge-1.16.5-36.2.39.jar", b"PK\x03\x04".as_slice()),
                ("start.bat", b"@echo off".as_slice()),
            ],
        );
        let generated = vec!["start.bat".to_string()];
        let expected = vec!["a.jar".to_string()];
        let checks = run(&Input {
            staging: &dir,
            options: &ConversionOptions::default(),
            loader: LoaderKind::Forge,
            plan: &[],
            start_jar: Some("forge-1.16.5-36.2.39.jar"),
            args_files: &[],
            installed: true,
            generated: &generated,
            expected_mod_files: &expected,
            expected_keep_dirs: &HashMap::new(),
        });
        assert_eq!(status_of(&checks, "start"), CheckStatus::Pass);
        assert_eq!(status_of(&checks, "loader"), CheckStatus::Pass);
        let _ = fs::remove_dir_all(&dir);
    }

    /// 关着本机安装：包里只有安装器，首启才装出加载器。这不是缺陷，所以「Loader 就位」走提示档，
    /// 但话要说清——「自检通过」不等于「上传即跑」
    #[test]
    fn not_installed_pack_marks_loader_pending() {
        let dir = temp_staging(
            &[("mods/a.jar", b"")],
            &[
                ("forge-1.20.1-47.4.10-installer.jar", b"PK\x03\x04".as_slice()),
                ("start.bat", b"@echo off".as_slice()),
            ],
        );
        let generated = vec!["start.bat".to_string()];
        let expected = vec!["a.jar".to_string()];
        let checks = run(&Input {
            staging: &dir,
            options: &ConversionOptions::default(),
            loader: LoaderKind::Forge,
            plan: &[],
            start_jar: Some("forge-1.20.1-47.4.10-installer.jar"),
            args_files: &[],
            installed: false,
            generated: &generated,
            expected_mod_files: &expected,
            expected_keep_dirs: &HashMap::new(),
        });
        assert_eq!(status_of(&checks, "start"), CheckStatus::Pass);
        let loader = checks.iter().find(|c| c.id == "loader").unwrap();
        assert_eq!(loader.status, CheckStatus::Warn, "{}", loader.detail);
        assert!(loader.detail.contains("联网"), "{}", loader.detail);
        let _ = fs::remove_dir_all(&dir);
    }
}
