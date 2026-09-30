//! 服务端包组装：在 staging 目录写入脚本/配置文件，打包为 {name}-server.zip。

use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use thiserror::Error;
use zip::write::SimpleFileOptions;
use zip::CompressionMethod;

use crate::core::installer::Installed;
use crate::core::mc_version;
use crate::models::{ConversionOptions, LoaderKind};
use run_script::{
    jvm_args, one_line, one_line_escaped, props_must_escape, resolve_shape, RunShape,
};
use zip_writer::{collect, write_zip, Planned};

mod run_script;
mod zip_writer;

#[derive(Error, Debug)]
pub enum BuilderError {
    #[error("文件写入失败：{0}")]
    Io(#[from] std::io::Error),
    #[error("打包失败：{0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("本机装好的加载器认不出启动方式：{0}")]
    Layout(String),
}

pub struct BuildInput<'a> {
    /// 已包含 mods/、config/、服务端 jar 的暂存目录
    pub staging: &'a Path,
    pub output_dir: &'a Path,
    pub output_file_name: String,
    /// 本任务上一次的产物绝对路径（重试时传入）：撞名时覆写自己那份而不是加序号
    pub own_output: Option<PathBuf>,
    pub options: &'a ConversionOptions,
    pub loader: LoaderKind,
    /// Fabric：官方服务端 jar 文件名（首启自装）；Forge/NeoForge 为 None（用 installer + run 脚本）
    pub server_jar_name: Option<String>,
    /// Forge/NeoForge installer jar 文件名
    pub installer_jar_name: Option<String>,
    /// 阶段 2.5 在本机装好的 loader 树：Some ⇒ 这棵树并进 staging、安装器 jar 不再进包、
    /// 启动脚本直接指向装好的参数文件；None ⇒ 一切照旧（包里留 installer，首启自装）
    pub installed: Option<&'a Installed>,
    /// 写入包根的说明文件名（可空）
    pub readme_lines: Vec<String>,
}

/// 构建产物与可展示的步骤信息（流水线据此打日志）
pub struct BuildReport {
    pub path: PathBuf,
    pub size: u64,
    /// 写入包根的文件名（脚本/协议/属性/说明）
    pub generated: Vec<String>,
    /// 打进 zip 的文件数
    pub entries: usize,
    /// 覆写了本任务上一次的产物（同名序号只给别人的包）
    pub overwritten: bool,
    /// 启动脚本指向的 jar 名（报告页据此写出真实的手动启动命令）。
    /// 已装的新式布局为 None：它靠 `libraries/` 下的参数文件启动，没有单一 jar 可指
    pub start_jar: Option<String>,
    /// 新式已装布局的两份参数文件（包内相对路径，POSIX 分隔）。`start_jar` 为 None 时"启动指向"对账的就是它们——
    /// 自检不能只信 builder 自己算的结论，所以这里把事实交出去，让 `verify` 对着 staging 再查一遍落盘没有
    pub args_files: Vec<String>,
    /// 本次把本机装好的 loader 树并进了产物（目标机不需要再联网首装）
    pub installed: bool,
    /// 包内自带、因此本次**没有**按配置生成的包根文件（用户在保留内容里勾了同名条目 ⇒ 以包内那份为准）。
    /// 与 `generated` 是互斥的两半：配置卡里对应的值这次不会进产物，报告和界面都得说实话
    pub reused_root: Vec<String>,
}

/// 打包过程事件：Plan 先给总量（实时条的分母），File 逐文件累加字节，
/// Group 在每个顶层目录写完时出一条日志
#[derive(Debug, Clone)]
pub enum BuildEvent {
    Plan { files: usize, bytes: u64 },
    /// group = 该条目所属顶层目录，实时条拿它当「正在打包什么」的文案
    File { group: String, bytes: u64 },
    Group { label: String, files: usize, bytes: u64 },
}


/// 输出名落地：默认名空着就用它；被别的包占了就 `{stem}-server-2.zip` 递增；
/// 递增到本任务自己上一份时回到那份（覆写，不留一堆重复包）
fn resolve_out(dir: &Path, desired: &str, own: Option<&Path>) -> PathBuf {
    let (stem, ext) = match desired.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (desired.to_string(), String::new()),
    };
    let mut n = 0usize;
    loop {
        // 序号从 2 起（-server-2.zip），0 号是默认名本身
        let name = if n == 0 {
            desired.to_string()
        } else {
            format!("{stem}-{}{ext}", n + 1)
        };
        let cand = dir.join(name);
        if own == Some(cand.as_path()) || !cand.exists() {
            return cand;
        }
        n += 1;
    }
}

pub fn build(
    input: &BuildInput,
    on_event: &mut dyn FnMut(&BuildEvent),
) -> Result<BuildReport, BuilderError> {
    // 启动形态先定再并树：认不出来就当场失败，不要先拷 150 MB 再告诉用户打包炸了
    let shape = resolve_shape(input.installed)?;
    if let Some(installed) = input.installed {
        merge_installed(&installed.dir, input.staging)?;
        // 安装器 jar 的唯一作用是首启自装，而首启已经不需要它了 ⇒ 从 staging 摘掉，别打进包
        if let Some(jar) = &input.installer_jar_name {
            let _ = std::fs::remove_file(input.staging.join(jar));
        }
    }
    let mut reused_root: Vec<String> = Vec::new();
    let generated = write_root_files(input, &shape, &mut reused_root)?;
    std::fs::create_dir_all(input.output_dir)?;
    let out_path = resolve_out(
        input.output_dir,
        &input.output_file_name,
        input.own_output.as_deref(),
    );
    let overwritten = out_path.exists();
    let file = File::create(&out_path)?;
    let mut zip = zip::ZipWriter::new(BufWriter::new(file));

    // 先列清单再写：总量是实时条的分母，也是「哪个顶层目录写完」的判据
    let mut plan: Vec<Planned> = Vec::new();
    collect(input.staging, input.staging, &mut plan)?;
    let total_bytes: u64 = plan.iter().map(|p| p.size).sum();
    on_event(&BuildEvent::Plan { files: plan.len(), bytes: total_bytes });

    let res = write_zip(&mut zip, &plan, on_event).and_then(|_| zip.finish().map_err(BuilderError::Zip));
    if res.is_err() {
        // 半截 zip 留在输出目录只会误导（它看着像上一次的成功产物），删掉再报错
        let _ = std::fs::remove_file(&out_path);
        res?;
    }
    let size = out_path.metadata().map(|m| m.len()).unwrap_or(0);
    Ok(BuildReport {
        path: out_path,
        size,
        generated,
        entries: plan.len(),
        overwritten,
        start_jar: match &shape {
            RunShape::ArgsFiles { .. } => None,
            RunShape::LegacyJar { jar } => Some(jar.clone()),
            RunShape::FirstBootInstall => input
                .server_jar_name
                .clone()
                .or_else(|| input.installer_jar_name.clone()),
        },
        args_files: match &shape {
            RunShape::ArgsFiles { win, unix } => vec![win.clone(), unix.clone()],
            _ => Vec::new(),
        },
        installed: input.installed.is_some(),
        reused_root,
    })
}



/// 安装目录里不进交付包的东西（实测 Forge 1.20.1 / NeoForge 26.2 的新式布局与 1.16.5 的老式布局）：
/// run 脚本与 JVM 参数模板由 builder 自己生成，`inst.sha1` 是安装器自留的记账
const INSTALLER_KEEP_OUT: &[&str] = &[
    "run.bat",
    "run.sh",
    "user_jvm_args.txt",
    "inst.sha1",
    "eula.txt",
    "server.properties",
];
/// 运行期目录：服务端首启才会造（安装目录被手动跑过一次就有）。包里的 mods/ 与 config/
/// 归取件阶段管，让安装目录里那份盖过来等于把上一次试启动的残留打进交付包
const INSTALLER_KEEP_OUT_DIRS: &[&str] = &["config", "defaultconfigs", "logs", "mods"];

/// 安装目录里的这个相对路径（POSIX 分隔、小写）是否该被挡在交付包外
fn installed_kept_out(rel: &str) -> bool {
    let lower = rel.to_ascii_lowercase();
    if lower.ends_with(".log") {
        return true;
    }
    // 安装器自己那枚 jar：这一档存在的意义就是「已经在本机跑过了」
    if lower.ends_with(".jar") && lower.contains("installer") {
        return true;
    }
    match lower.split_once('/') {
        None => INSTALLER_KEEP_OUT.contains(&lower.as_str()),
        // 只按顶层目录名挡：`libraries/` 里真有个 mods/ 那也是依赖的一部分
        Some((head, _)) => INSTALLER_KEEP_OUT_DIRS.contains(&head),
    }
}

/// 把装好的 loader 树并进 staging。staging 必须等于产物内容（自检与「打开目录」都按这个前提读），
/// 所以这里复制而不是打包时外挂引用。已有同路径文件一律不覆写：包的 mods/ 与 config/ 由取件阶段说了算
fn merge_installed(src: &Path, staging: &Path) -> Result<(), BuilderError> {
    let mut dirs = vec![src.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        for e in std::fs::read_dir(&dir)? {
            let path = e?.path();
            let rel = path
                .strip_prefix(src)
                .map_err(|e| BuilderError::Io(std::io::Error::other(e)))?
                .to_string_lossy()
                .replace('\\', "/");
            if installed_kept_out(&rel) {
                continue;
            }
            if path.is_dir() {
                dirs.push(path);
                continue;
            }
            let to = staging.join(&rel);
            if to.exists() {
                continue;
            }
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(&path, &to)?;
        }
    }
    Ok(())
}


/// 写一份包根交付文件。**包内已自带同名文件（用户在「客户端保留内容」里勾了它）就以包内那份为准**：
/// 勾了就是要自己那份，再按配置写一遍等于把用户的决定覆盖掉。跳过的那枚记进 `reused`，
/// 由流水线打日志、由报告与界面说明「这一项本次没按配置生成」
fn emit_root(
    staging: &Path,
    name: &str,
    body: impl FnOnce() -> String,
    generated: &mut Vec<String>,
    reused: &mut Vec<String>,
) -> Result<(), BuilderError> {
    let to = staging.join(name);
    if to.exists() {
        reused.push(name.to_string());
        return Ok(());
    }
    std::fs::write(to, body())?;
    generated.push(name.to_string());
    Ok(())
}

/// 启动脚本、eula、server.properties、README；返回本次实际生成的包根文件名
fn write_root_files(
    input: &BuildInput,
    shape: &RunShape,
    reused: &mut Vec<String>,
) -> Result<Vec<String>, BuilderError> {
    let mut generated: Vec<String> = Vec::new();
    let jvm = jvm_args(input.options);
    if input.options.generate_scripts {
        // 脚本先锚定自身目录：从任意 cwd 调用（终端/计划任务）相对 jar 路径仍然有效
        let nogui = if input.options.nogui { " nogui" } else { "" };
        let (bat, sh) = match input.loader {
            LoaderKind::Fabric => {
                let jar = input.server_jar_name.as_deref().unwrap_or("server.jar");
                (
                    format!(
                        "@echo off\r\ncd /d \"%~dp0\"\r\njava {jvm} -jar {jar}{nogui}\r\npause\r\n"
                    ),
                    format!("#!/usr/bin/env bash\ncd \"$(dirname \"$0\")\"\njava {jvm} -jar {jar}{nogui}\n"),
                )
            }
            LoaderKind::Forge | LoaderKind::NeoForge => {
                // 三态里两态靠 `@user_jvm_args.txt` 注入 JVM 参数（老 Forge 没有这套，参数上命令行）
                if !matches!(shape, RunShape::LegacyJar { .. }) {
                    emit_root(
                        input.staging,
                        "user_jvm_args.txt",
                        || format!("{jvm}\n"),
                        &mut generated,
                        reused,
                    )?;
                }
                match shape {
                    // 已装好：直接引用安装器生成的参数文件（内部全是相对路径，整棵树可整体搬运）
                    RunShape::ArgsFiles { win, unix } => (
                        format!(
                            "@echo off\r\ncd /d \"%~dp0\"\r\njava @user_jvm_args.txt @{win}{nogui}\r\npause\r\n"
                        ),
                        format!(
                            "#!/usr/bin/env bash\ncd \"$(dirname \"$0\")\"\njava @user_jvm_args.txt @{unix}{nogui}\n"
                        ),
                    ),
                    RunShape::LegacyJar { jar } => (
                        format!(
                            "@echo off\r\ncd /d \"%~dp0\"\r\njava {jvm} -jar {jar}{nogui}\r\npause\r\n"
                        ),
                        format!("#!/usr/bin/env bash\ncd \"$(dirname \"$0\")\"\njava {jvm} -jar {jar}{nogui}\n"),
                    ),
                    // 未本机安装：首次运行自动执行 installServer，之后用 installer 生成的 run 脚本启动。
                    // 开关必须转给那两份脚本（实测它们以 `%*` / `"$@"` 收尾）——不带过去
                    // 等于这一档的「无界面」静默失效，另三态都是自己在命令行上带参数的
                    RunShape::FirstBootInstall => {
                        let installer = input
                            .installer_jar_name
                            .as_deref()
                            .unwrap_or("installer.jar");
                        (
                            format!(
                                "@echo off\r\ncd /d \"%~dp0\"\r\nif not exist run.bat java -Xmx1G -jar {installer} --installServer\r\nif exist run.bat (call run.bat{nogui}) else (echo 安装失败，请手动运行: java -jar {installer} --installServer & pause)\r\n"
                            ),
                            format!(
                                "#!/usr/bin/env bash\ncd \"$(dirname \"$0\")\"\n[ -f run.sh ] || java -Xmx1G -jar {installer} --installServer\nbash run.sh{nogui}\n"
                            ),
                        )
                    }
                }
            }
        };
        emit_root(input.staging, "start.bat", move || bat, &mut generated, reused)?;
        emit_root(input.staging, "start.sh", move || sh, &mut generated, reused)?;
    }
    // eula.txt 按开关生成（false 时服务端拒启，用户按 README 手改 true）
    emit_root(
        input.staging,
        "eula.txt",
        || {
            format!(
            "# eula=true 表示同意 Mojang 服务端最终用户协议（由 SideShift 按开关写入）\neula={}\n",
            if input.options.agree_eula { "true" } else { "false" }
        )
        },
        &mut generated,
        reused,
    )?;
    // 同一条规矩：包内自带 `server.properties` 就整份沿用。我们只写界面上那 8 项 + 模板默认值，
    // 覆盖过去会丢掉包内没在界面上暴露的键（`level-type` / `generator-settings` / `view-distance` …）
    if input.staging.join("server.properties").exists() {
        reused.push("server.properties".to_string());
    } else {
        let o = input.options;
        // 枚举字段白名单收口，防脏值写入属性文件
        let gamemode = match o.gamemode.as_str() {
            "creative" | "adventure" | "spectator" => o.gamemode.as_str(),
            _ => "survival",
        };
        let difficulty = match o.difficulty.as_str() {
            "peaceful" | "normal" | "hard" => o.difficulty.as_str(),
            _ => "easy",
        };
        let mut props = String::from("# 由 SideShift 按转换配置生成，可按需修改\n");
        // 只有**用户打的字**过这道闸门：目标服是 1.20 以前时非 ASCII 必须转义（判据见 props_must_escape）
        let text = |s: &str| {
            if props_must_escape(&o.mc_version) {
                one_line_escaped(s)
            } else {
                one_line(s)
            }
        };
        // UI 开关/下拉驱动的高频字段
        props.push_str(&format!("online-mode={}\n", o.online_mode));
        props.push_str(&format!("server-port={}\n", o.server_port));
        props.push_str(&format!("motd={}\n", text(&o.motd)));
        props.push_str(&format!("max-players={}\n", o.max_players));
        props.push_str(&format!("gamemode={gamemode}\n"));
        props.push_str(&format!("difficulty={difficulty}\n"));
        if !o.level_seed.trim().is_empty() {
            props.push_str(&format!("level-seed={}\n", text(o.level_seed.trim())));
        }
        // 其余按 vanilla 常用默认值给全（缺失键服务端首启也会自动补齐，这里给的是可读的完整模板）。
        // 模板是 1.20–1.21.1 那一份键表：不认识的键服务端首启重写文件时会自行丢掉、缺的键补该版本默认，
        // 所以键的增减都无害 —— 有害的只有**值**：`level-type` 因此不写。它在 1.19(22w11a) 起改收
        // world preset ID（vanilla 自己写 `minecraft\:normal`），恒写老值 `default` 等于赌它被兼容映射
        props.push_str(&format!(
            "\
level-name=world
server-ip=
pvp=true
allow-flight=false
allow-nether=true
white-list=false
enforce-whitelist=false
hardcore=false
force-gamemode=false
generate-structures=true
spawn-npcs=true
spawn-animals=true
spawn-monsters=true
spawn-protection=16
enable-command-block=false
function-permission-level=2
op-permission-level=4
network-compression-threshold=256
player-idle-timeout=0
max-tick-time=60000
entity-broadcast-range-percentage=100
sync-chunk-writes=true
use-native-transport=true
prevent-proxy-connections=false
enable-status=true
broadcast-console-to-ops=true
broadcast-rcon-to-ops=true
enable-jmx-monitoring=false
log-ips=true
snooper-enabled=true
enable-query=false
query.port={}
enable-rcon=false
rcon.port=25575
rcon.password=
resource-pack=
resource-pack-sha1=
require-resource-pack=false
initial-enabled-packs=vanilla
initial-disabled-packs=
text-filtering-config=
view-distance=10
simulation-distance=10
",
            o.server_port
        ));
        std::fs::write(input.staging.join("server.properties"), props)?;
        generated.push("server.properties".to_string());
    }
    if !input.readme_lines.is_empty() {
        emit_root(
            input.staging,
            "README-SideShift.txt",
            || input.readme_lines.join("\n") + "\n",
            &mut generated,
            reused,
        )?;
    }
    Ok(generated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::zip_writer::{group_of, stored_for};

    fn opts() -> ConversionOptions {
        ConversionOptions {
            mc_version: "1.20.1".into(),
            loader_version: "0.15.3".into(),
            java_version: "17".into(),
            memory_mb: 4096,
            generate_scripts: true,
            nogui: true,
            agree_eula: false,
            server_port: 25565,
            motd: "test".into(),
            max_players: 20,
            gamemode: "survival".into(),
            difficulty: "easy".into(),
            online_mode: true,
            level_seed: String::new(),
            use_aikar_flags: false,
            extra_jvm_args: String::new(),
            output_override: String::new(),
            keep_dirs: vec![],
            // 其余字段铺 Default：把这份夹具写成全字段枚举，加一个 option 就得跟着补一处
            ..Default::default()
        }
    }

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("sideshift-build-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn put(path: &Path, bytes: &[u8]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    fn input<'a>(staging: &'a Path, out: &'a Path, o: &'a ConversionOptions) -> BuildInput<'a> {
        BuildInput {
            staging,
            output_dir: out,
            output_file_name: "pack-server.zip".into(),
            own_output: None,
            options: o,
            loader: LoaderKind::Fabric,
            server_jar_name: Some("server.jar".into()),
            installer_jar_name: None,
            installed: None,
            readme_lines: vec![],
        }
    }

    #[test]
    fn stored_only_for_already_compressed_suffixes() {
        assert!(stored_for("mods/big-mod.jar"));
        assert!(stored_for("resources/SOUND.OGG"), "后缀大小写不敏感");
        assert!(!stored_for("config/a.toml"));
        assert!(!stored_for("start.sh"));
        assert!(!stored_for("README"));
    }

    #[test]
    fn group_label_follows_first_folder_and_renames_mods() {
        assert_eq!(group_of("mods/x.jar"), "模组");
        assert_eq!(group_of("Mods/x.jar"), "模组");
        assert_eq!(group_of("kubejs/client/x.js"), "kubejs");
        assert_eq!(group_of("eula.txt"), "根文件");
    }

    #[test]
    fn other_tasks_pack_gets_suffix_but_retry_reuses_own_slot() {
        let dir = tmp();
        let desired = "pack-server.zip";
        assert_eq!(resolve_out(&dir, desired, None), dir.join(desired));
        put(&dir.join(desired), b"another task");
        // 默认名被别人的包占了 → 加序号，绝不静默覆写
        let second = resolve_out(&dir, desired, None);
        assert_eq!(second, dir.join("pack-server-2.zip"));
        put(&second, b"mine");
        // 本任务重试：认得自己那份，回到 -2 而不是又造一个 -3
        assert_eq!(resolve_out(&dir, desired, Some(&second)), second);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn build_emits_plan_files_groups_and_splits_compression() {
        let root = tmp();
        let staging = root.join("staging");
        let out = root.join("out");
        let jarish: Vec<u8> = (0..40_000u32).map(|i| (i % 253) as u8).collect();
        put(&staging.join("mods/example-mod.jar"), &jarish);
        put(&staging.join("config/settings.toml"), b"key = \"value\"\n");
        put(&staging.join("config/nested/inner.txt"), b"hello");

        let o = opts();
        let mut events: Vec<BuildEvent> = Vec::new();
        let report = build(&input(&staging, &out, &o), &mut |e| events.push(e.clone())).unwrap();
        assert!(!report.overwritten, "首次构建不该报覆写");
        assert!(report.path.exists());

        let planned = match events.first().cloned() {
            Some(BuildEvent::Plan { files, bytes }) => (files, bytes),
            other => panic!("首个事件应为 Plan，实得 {other:?}"),
        };
        assert_eq!(planned.0, report.entries);
        let file_bytes: u64 = events
            .iter()
            .filter_map(|e| match e {
                BuildEvent::File { bytes, .. } => Some(*bytes),
                _ => None,
            })
            .sum();
        assert_eq!(file_bytes, planned.1, "逐文件字节累加应等于 Plan 总量");
        let groups: Vec<&str> = events
            .iter()
            .filter_map(|e| match e {
                BuildEvent::Group { label, .. } => Some(label.as_str()),
                _ => None,
            })
            .collect();
        assert!(groups.contains(&"模组"), "实得 {groups:?}");
        assert!(groups.contains(&"config"), "实得 {groups:?}");
        assert!(groups.contains(&"根文件"), "实得 {groups:?}");

        // 分流压缩：jar 走 Stored（已是 deflate，再压纯烧 CPU），文本走 Deflated；
        // .sh 补执行位
        let mut z = zip::ZipArchive::new(File::open(&report.path).unwrap()).unwrap();
        assert_eq!(
            z.by_name("mods/example-mod.jar").unwrap().compression(),
            CompressionMethod::Stored
        );
        assert_eq!(
            z.by_name("config/settings.toml").unwrap().compression(),
            CompressionMethod::Deflated
        );
        assert_eq!(
            z.by_name("start.sh").unwrap().unix_mode().unwrap() & 0o755,
            0o755,
            "shell 脚本必须带执行位"
        );

        // 同名第二个任务：加序号；本任务重试：认得自己那份并覆写
        let second = build(&input(&staging, &out, &o), &mut |_| {}).unwrap();
        assert_eq!(second.path, out.join("pack-server-2.zip"));
        assert!(!second.overwritten, "序号位是空出来的，不叫覆写");
        let mut i2 = input(&staging, &out, &o);
        i2.own_output = Some(second.path.clone());
        let retry = build(&i2, &mut |_| {}).unwrap();
        assert_eq!(retry.path, second.path, "重试应覆写自己那份而不是再递增");
        assert!(retry.overwritten);
        assert_eq!(
            std::fs::read_dir(&out).unwrap().count(),
            2,
            "同一任务反复重试不该攒出一堆重复包"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /* ---------------- 包根撞车：勾了就以包内那份为准 ---------------- */

    /// 用户在保留内容里勾了根文件 ⇒ staging 里已经有了（流水线按 keep 清单拷进来的），
    /// 这时 builder 不能再按配置写一遍。eula 走开关、server.properties 走那 8 项，
    /// 覆盖等于把用户的决定抹掉
    #[test]
    fn kept_eula_wins_and_the_rest_still_get_generated() {
        let root = tmp();
        let staging = root.join("staging");
        let out = root.join("out");
        put(&staging.join("mods/example-mod.jar"), b"PK\x03\x04");
        // agree_eula 是 false，所以只要我们那一份就会写成 eula=false
        put(&staging.join("eula.txt"), b"eula=true\n");

        let o = opts();
        let report = build(&input(&staging, &out, &o), &mut |_| {}).unwrap();

        assert_eq!(report.reused_root, vec!["eula.txt".to_string()], "{:?}", report.reused_root);
        assert!(
            !report.generated.contains(&"eula.txt".to_string()),
            "勾了的不能再算生成：{:?}",
            report.generated
        );
        // 没勾的那两枚照旧按配置生成
        assert!(report.generated.contains(&"server.properties".to_string()));
        assert!(report.generated.contains(&"start.bat".to_string()));
        assert_eq!(
            zip_text(&report.path, "eula.txt"),
            "eula=true\n",
            "包里必须是用户那份字节，不是开关算出来的 false"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// server.properties 是整份沿用、不是按键合并：builder 只认识界面上那 8 项 + 模板，
    /// 包内那些没暴露的键（level-type / generator-settings / view-distance）一旦被覆盖就永久丢了
    #[test]
    fn kept_server_properties_is_carried_whole_not_merged() {
        let root = tmp();
        let staging = root.join("staging");
        let out = root.join("out");
        put(&staging.join("mods/example-mod.jar"), b"PK\x03\x04");
        put(
            &staging.join("server.properties"),
            b"level-type=minecraft\\:large_biomes\nmax-players=99\n",
        );

        let o = opts();
        let report = build(&input(&staging, &out, &o), &mut |_| {}).unwrap();

        assert_eq!(report.reused_root, vec!["server.properties".to_string()]);
        let props = zip_text(&report.path, "server.properties");
        assert!(props.contains("large_biomes"), "包内独有的键要原样带过：{props}");
        assert!(
            !props.contains("server-port"),
            "沿用 = 整份照搬，不是把配置项并进去：{props}"
        );
        // eula 没勾 → 这一枚仍然按开关生成
        assert!(report.generated.contains(&"eula.txt".to_string()));
        let _ = std::fs::remove_dir_all(&root);
    }

    /* ---------------- 本机安装的并树与启动脚本三分叉 ---------------- */

    use crate::core::installer;

    /// 安装目录夹具：`files` 按真实布局给，scripts/jars 是 installer 那边探测出来的两份顶层名单
    fn installed_tree(files: &[(&str, &[u8])], scripts: &[&str], jars: &[&str]) -> Installed {
        let dir = tmp().join("installs/forge/1.20.1-47.4.10");
        for (rel, body) in files {
            put(&dir.join(rel), body);
        }
        Installed {
            dir,
            from_cache: true,
            report: installer::InstallReport {
                files: files.len() as u64,
                bytes: 0,
                elapsed: std::time::Duration::ZERO,
                scripts: scripts.iter().map(|s| s.to_string()).collect(),
                jars: jars.iter().map(|s| s.to_string()).collect(),
            },
        }
    }

    fn zip_names(path: &Path) -> Vec<String> {
        let z = zip::ZipArchive::new(File::open(path).unwrap()).unwrap();
        let mut v: Vec<String> = z.file_names().map(|s| s.to_string()).collect();
        v.sort();
        v
    }

    fn zip_text(path: &Path, name: &str) -> String {
        use std::io::Read;
        let mut z = zip::ZipArchive::new(File::open(path).unwrap()).unwrap();
        let mut s = String::new();
        z.by_name(name).unwrap().read_to_string(&mut s).unwrap();
        s
    }

    fn forge_input<'a>(
        staging: &'a Path,
        out: &'a Path,
        o: &'a ConversionOptions,
        installed: &'a Installed,
    ) -> BuildInput<'a> {
        BuildInput {
            loader: LoaderKind::Forge,
            installer_jar_name: Some("forge-installer.jar".into()),
            installed: Some(installed),
            ..input(staging, out, o)
        }
    }

    /// 未本机安装那一档：start 脚本只负责首启自装，之后转 call installer 生成的 run 脚本，
    /// 所以「无界面」必须往下带（那两份脚本实测以 `%*` / `"$@"` 收尾，见下一个用例里的 run 脚本样本）
    #[test]
    fn first_boot_install_forwards_the_switches_to_run_scripts() {
        let root = tmp();
        let staging = root.join("staging");
        let out = root.join("out");
        put(&staging.join("mods/example-mod.jar"), b"PK\x03\x04");

        let o = opts();
        let mut i = input(&staging, &out, &o);
        i.loader = LoaderKind::Forge;
        i.server_jar_name = None;
        i.installer_jar_name = Some("forge-installer.jar".into());
        let report = build(&i, &mut |_| {}).unwrap();
        let bat = zip_text(&report.path, "start.bat");
        let sh = zip_text(&report.path, "start.sh");
        assert!(bat.contains("call run.bat nogui"), "{bat}");
        assert!(sh.contains("bash run.sh nogui"), "{sh}");
        // 属性文件里那个可能无效的值不写（1.19 起 level-type 收 world preset ID，缺键由服务端补自家默认）
        let props = zip_text(&report.path, "server.properties");
        assert!(!props.contains("level-type"), "{props}");
        assert!(props.contains("level-name=world"), "{props}");

        // 关掉「无界面」就该原样不带参数，而不是恒塞 nogui。
        // 换一份 staging：包根文件只在缺席时才生成，共用 staging 的话这里复用的是上一步自己写的脚本
        // （生产里不会撞上——流水线每次跑先清空 staging）
        let staging2 = root.join("staging-2");
        put(&staging2.join("mods/example-mod.jar"), b"PK\x03\x04");
        let mut off = opts();
        off.nogui = false;
        let mut i2 = input(&staging2, &out, &off);
        i2.loader = LoaderKind::Forge;
        i2.server_jar_name = None;
        i2.installer_jar_name = Some("forge-installer.jar".into());
        let second = build(&i2, &mut |_| {}).unwrap();
        let bat = zip_text(&second.path, "start.bat");
        assert!(bat.contains("call run.bat)") || bat.contains("call run.bat ("), "{bat}");
        assert!(!bat.contains("nogui"), "{bat}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 属性文件的编码闸门：1.20 以下的服务端按 ISO-8859-1 读 ⇒ 中文要转 `\uXXXX`；
    /// 1.20 起是 UTF-8 ⇒ 原样落盘，保住文件可读（两档都是转义与否，值本身不变）
    #[test]
    fn prop_values_escape_only_for_latin1_servers() {
        let root = tmp();
        let out = root.join("out");

        // 每次构建各用一个 staging：`server.properties` 只在它不存在时才生成，共用一份 staging 会让
        // 第二次构建直接拿掉上辈子那一份（测出来就是"新旧档一个样"）
        let props_for = |mc: &str| {
            let staging = root.join(format!("staging-{mc}"));
            put(&staging.join("mods/example-mod.jar"), b"PK\x03\x04");
            let o = ConversionOptions {
                mc_version: mc.into(),
                motd: "中文服".into(),
                level_seed: "种子1".into(),
                ..opts()
            };
            let report = build(&input(&staging, &out, &o), &mut |_| {}).unwrap();
            zip_text(&report.path, "server.properties")
        };
        let old = props_for("1.16.5");
        assert!(old.contains("motd=\\u4E2D\\u6587\\u670D"), "{old}");
        assert!(old.contains("level-seed=\\u79CD\\u5B501"), "{old}");
        let new = props_for("1.20.1");
        assert!(new.contains("motd=中文服"), "{new}");
        assert!(new.contains("level-seed=种子1"), "{new}");
        // 转义档也一样不写那个可能无效的值
        assert!(!old.contains("level-type"), "{old}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn latin1_verdict_covers_both_numbering_schemes() {
        // 边界就在 1.20：pre1 才换的读法，1.19.4 还在旧侧
        assert!(props_must_escape("1.19.4"));
        assert!(props_must_escape("1.12.2"));
        assert!(props_must_escape("1.8.9"));
        assert!(!props_must_escape("1.20"));
        assert!(!props_must_escape("1.20.1"));
        assert!(!props_must_escape("1.21.1"));
        // 年份写法（26.3 这类）与 1.x 不同源，首段 >1 一律算新侧
        assert!(!props_must_escape("26.3"));
        // 判不出来的串走"转"那一边：`\uXXXX` 在新服务端照样解得对，反过来会真乱码
        assert!(props_must_escape("24w14a"));
        assert!(props_must_escape("b1.7.3"));
        assert!(props_must_escape(""));
    }

    #[test]
    fn escaping_leaves_ascii_and_doubles_backslash_before_counting() {
        assert_eq!(one_line_escaped("A server 42"), "A server 42");
        // 先按 one_line 把 `\` 翻倍，再只对非 ASCII 出手：`\` 本身是 ASCII，不该被转义掉
        assert_eq!(one_line_escaped("中\\A"), "\\u4E2D\\\\A");
        // 星平面字符没有单个 `\uXXXX` 装得下，必须成对代理
        assert_eq!(one_line_escaped("🧱"), "\\uD83E\\uDDF1");
    }

    /// 新式布局（实测 Forge 1.20.1、NeoForge 26.2）：依赖树进包、installer 的 run 脚本与记账件不进包，
    /// start 脚本自己生成并引用装好的参数文件
    #[test]
    fn installed_new_style_ships_tree_and_points_at_args_files() {
        let root = tmp();
        let staging = root.join("staging");
        let out = root.join("out");
        let args = "libraries/net/minecraftforge/forge/1.20.1-47.4.10";
        let win_line =
            "java @user_jvm_args.txt @libraries/net/minecraftforge/forge/1.20.1-47.4.10/win_args.txt %*";
        let unix_line =
            "java @user_jvm_args.txt @libraries/net/minecraftforge/forge/1.20.1-47.4.10/unix_args.txt \"$@\"";
        let installed = installed_tree(
            &[
                (&format!("{args}/win_args.txt"), b"--launchTarget forgeserver"),
                (&format!("{args}/unix_args.txt"), b"--launchTarget forgeserver"),
                ("libraries/net/minecraft/server/server-1.20.1-extra.jar", b"PK\x03\x04"),
                ("libraries/keep.txt", b"theirs"),
                ("run.bat", format!("@echo off\r\n{win_line}\r\npause\r\n").as_bytes()),
                ("run.sh", format!("#!/usr/bin/env sh\n{unix_line}\n").as_bytes()),
                ("user_jvm_args.txt", b"# -Xmx4G\n"),
                ("install.log", b"noise"),
                ("forge-installer.jar.log", b"noise"),
                ("inst.sha1", b"deadbeef"),
                ("config/fml.toml", b"runDirectory = '.'\n"),
                ("mods/readme.txt", "服务端首启造的".as_bytes()),
                ("eula.txt", b"eula=true\n"),
            ],
            &["run.bat", "run.sh"],
            &[],
        );
        put(&staging.join("mods/example-mod.jar"), b"PK\x03\x04");
        put(&staging.join("forge-installer.jar"), b"installer bytes");
        // 同路径文件以 staging 为准：安装目录只补依赖，不抢包自己那份
        put(&staging.join("libraries/keep.txt"), b"ours");

        let o = opts();
        let report = build(&forge_input(&staging, &out, &o, &installed), &mut |_| {}).unwrap();
        assert!(report.installed);
        assert_eq!(report.start_jar, None, "新式布局靠参数文件启动，没有单一 jar 可指");

        let names = zip_names(&report.path);
        assert!(names.contains(&format!("{args}/win_args.txt")), "{names:?}");
        assert!(names.contains(&format!("{args}/unix_args.txt")), "{names:?}");
        assert!(
            names.contains(&"libraries/net/minecraft/server/server-1.20.1-extra.jar".to_string()),
            "离线可跑全靠 libraries/：{names:?}"
        );
        assert_eq!(zip_text(&report.path, "libraries/keep.txt"), "ours");
        for gone in [
            "run.bat",
            "run.sh",
            "inst.sha1",
            "install.log",
            "forge-installer.jar.log",
            "config/fml.toml",
            "mods/readme.txt",
            "forge-installer.jar",
        ] {
            assert!(!names.contains(&gone.to_string()), "{gone} 不该进交付包");
        }
        // 脚本：引用参数文件，不再提 installServer，也不 call 那份带 pause 的 run.bat
        let bat = zip_text(&report.path, "start.bat");
        assert!(
            bat.contains(&format!("java @user_jvm_args.txt @{args}/win_args.txt nogui")),
            "{bat}"
        );
        assert!(!bat.contains("installServer") && !bat.contains("run.bat"), "{bat}");
        let sh = zip_text(&report.path, "start.sh");
        assert!(
            sh.contains(&format!("java @user_jvm_args.txt @{args}/unix_args.txt nogui")),
            "{sh}"
        );
        // installer 那两份模板/协议文件由我们的内容盖掉
        assert!(zip_text(&report.path, "user_jvm_args.txt").contains("-Xmx4096M"));
        assert!(zip_text(&report.path, "eula.txt").contains("eula=false"));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 老 Forge（实测 1.16.5）：装完没有 run 脚本与参数文件，顶层 universal jar 的 MANIFEST
    /// 自带相对 Class-Path ⇒ 一句 `java -jar <jar> nogui`，且不造没人读的 user_jvm_args.txt
    #[test]
    fn installed_old_forge_starts_from_the_universal_jar() {
        let root = tmp();
        let staging = root.join("staging");
        let out = root.join("out");
        let installed = installed_tree(
            &[
                ("libraries/net/minecraftforge/forge/1.16.5-36.2.39/forge-1.16.5-36.2.39.jar", b"PK\x03\x04"),
                ("forge-1.16.5-36.2.39.jar", b"PK\x03\x04"),
                ("minecraft_server.1.16.5.jar", b"PK\x03\x04"),
                ("forge-1.16.5-36.2.39-installer.jar", b"PK\x03\x04"),
                ("install.log", b"noise"),
            ],
            &[],
            &[
                "forge-1.16.5-36.2.39-installer.jar",
                "forge-1.16.5-36.2.39.jar",
                "minecraft_server.1.16.5.jar",
            ],
        );
        put(&staging.join("mods/example-mod.jar"), b"PK\x03\x04");
        put(&staging.join("forge-installer.jar"), b"installer bytes");

        let o = opts();
        let report = build(&forge_input(&staging, &out, &o, &installed), &mut |_| {}).unwrap();
        assert_eq!(
            report.start_jar.as_deref(),
            Some("forge-1.16.5-36.2.39.jar"),
            "既不能挑官方本体 jar，也不能挑根本不打进包的安装器 jar"
        );
        let names = zip_names(&report.path);
        assert!(names.contains(&"forge-1.16.5-36.2.39.jar".to_string()), "{names:?}");
        assert!(names.contains(&"minecraft_server.1.16.5.jar".to_string()), "{names:?}");
        assert!(!names.contains(&"forge-1.16.5-36.2.39-installer.jar".to_string()), "{names:?}");
        assert!(!names.contains(&"user_jvm_args.txt".to_string()), "老布局没有 @参数文件可读");
        let bat = zip_text(&report.path, "start.bat");
        assert!(
            bat.contains("java -Xmx4096M -jar forge-1.16.5-36.2.39.jar nogui"),
            "{bat}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 认不出布局就当场失败：这一档的用户要的是「上传即跑」，悄悄给一份还得联网首装的包比报错更坏
    #[test]
    fn unrecognized_installed_layout_fails_instead_of_degrading() {
        let o = opts();
        // run 脚本在，但没引用任何参数文件（安装器改了布局）
        let no_args = installed_tree(
            &[("run.bat", b"@echo off\r\njava -jar server.jar\r\n"), ("libraries/a.jar", b"x")],
            &["run.bat"],
            &[],
        );
        // 顶层既无脚本也无散 jar
        let bare = installed_tree(&[("libraries/a.jar", b"x")], &[], &[]);
        // 脚本指了一条不存在的参数文件路径
        let stale = installed_tree(
            &[("run.bat", b"java @user_jvm_args.txt @libraries/gone/win_args.txt %*\r\n")],
            &["run.bat"],
            &[],
        );
        for (case, why) in [(&no_args, "没有参数文件引用"), (&bare, "顶层空"), (&stale, "引用落空")] {
            let root = tmp();
            let staging = root.join("staging");
            std::fs::create_dir_all(&staging).unwrap();
            let err = build(&forge_input(&staging, &root.join("out"), &o, case), &mut |_| {})
                .err()
                .unwrap_or_else(|| panic!("{why} 的布局本该报错"));
            assert!(matches!(err, BuilderError::Layout(_)), "{why} ⇒ {err}");
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    /// 过滤表：安装目录里那些东西一件都不该出现在交付包（实测布局 + 手动试启动过的残留）
    #[test]
    fn installed_filter_blocks_runtime_leftovers() {
        for rel in [
            "run.bat",
            "run.sh",
            "user_jvm_args.txt",
            "inst.sha1",
            "eula.txt",
            "server.properties",
            "install.log",
            "logs/latest.log",
            "config/fml.toml",
            "defaultconfigs/x.toml",
            "mods/some.jar",
            "forge-installer.jar",
        ] {
            assert!(installed_kept_out(rel), "{rel} 应被挡住");
        }
        // 依赖本体与 libraries 里的同名嵌套目录都要照装：只按顶层目录名挡
        for keep in [
            "libraries/net/minecraftforge/forge/win_args.txt",
            "libraries/mods/inner.jar",
            "libraries/config/x.toml",
        ] {
            assert!(!installed_kept_out(keep), "{keep} 是依赖的一部分");
        }
    }

    /// 真机验：拿 `.scratch/installer-probe/` 里那三次真装出来的目录（Forge 1.20.1 新式 /
    /// NeoForge 26.2 新式 / Forge 1.16.5 老式，含我手动试启动留下的残留）跑一遍并树 + 分叉。
    /// 上面的夹具是按实测写的，这一条证明实测目录本身也过。跑法：
    /// `SS_INSTALLED_DIR=<安装目录绝对路径> cargo test --lib real_installed_tree -- --ignored --nocapture`
    #[test]
    #[ignore = "要指向一次真装出来的 loader 目录（几百 MB），手动跑"]
    fn real_installed_tree_merges_and_starts() {
        let src = PathBuf::from(std::env::var("SS_INSTALLED_DIR").expect("未给 SS_INSTALLED_DIR"));
        let mut scripts = Vec::new();
        let mut jars = Vec::new();
        for e in std::fs::read_dir(&src).unwrap().flatten() {
            if !e.path().is_file() {
                continue;
            }
            let name = e.file_name().to_string_lossy().to_string();
            let lower = name.to_ascii_lowercase();
            if lower == "run.bat" || lower == "run.sh" {
                scripts.push(name);
            } else if lower.ends_with(".jar") {
                jars.push(name);
            }
        }
        let installed = Installed {
            from_cache: true,
            report: installer::InstallReport {
                files: 0,
                bytes: 0,
                elapsed: std::time::Duration::ZERO,
                scripts,
                jars,
            },
            dir: src,
        };
        let root = tmp();
        let staging = root.join("staging");
        let out = root.join("out");
        put(&staging.join("mods/example-mod.jar"), b"PK\x03\x04");
        put(&staging.join("forge-installer.jar"), b"installer bytes");
        let o = opts();
        let report = build(&forge_input(&staging, &out, &o, &installed), &mut |_| {}).unwrap();
        let names = zip_names(&report.path);
        println!(
            "产物 {} · {} 个条目 · {} · start_jar={:?} · user_jvm_args 进包={}",
            report.path.display(),
            report.entries,
            report.size,
            report.start_jar,
            names.contains(&"user_jvm_args.txt".to_string())
        );
        println!("start.bat: {}", zip_text(&report.path, "start.bat"));
        // eula.txt 例外：包里有 ours 那份（builder 恒生成），这里挡的是安装目录那份
        for gone in ["run.bat", "run.sh", "install.log", "inst.sha1"] {
            assert!(!names.iter().any(|n| n == gone), "{gone} 不该进交付包");
        }
        assert!(!names.iter().any(|n| n.ends_with("-installer.jar")), "安装器 jar 不该进包");
        assert!(names.iter().any(|n| n.starts_with("libraries/")), "依赖树没并进包");
        assert!(!zip_text(&report.path, "start.bat").contains("installServer"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
