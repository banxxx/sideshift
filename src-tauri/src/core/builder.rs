//! 服务端包组装：在 staging 目录写入脚本/配置文件，打包为 {name}-server.zip。

use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use thiserror::Error;
use zip::write::SimpleFileOptions;
use zip::CompressionMethod;

use crate::models::{ConversionOptions, LoaderKind};

#[derive(Error, Debug)]
pub enum BuilderError {
    #[error("文件写入失败：{0}")]
    Io(#[from] std::io::Error),
    #[error("打包失败：{0}")]
    Zip(#[from] zip::result::ZipError),
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
    /// Fabric：一体化服务端 jar 文件名；Forge/NeoForge 为 None（用 installer + run 脚本）
    pub server_jar_name: Option<String>,
    /// Forge/NeoForge installer jar 文件名
    pub installer_jar_name: Option<String>,
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
    /// 启动脚本指向的 jar 名（报告页据此写出真实的手动启动命令）
    pub start_jar: Option<String>,
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

/// 已是压缩格式的后缀：jar/zip 本体就是 deflate，png/ogg 是有损压缩，
/// 再压一遍只烧 CPU 不省体积——单线程 Deflate 压几百 MB mod 就是「构建特别慢」的全部原因
const STORED_EXTS: &[&str] = &[
    "jar", "zip", "png", "jpg", "jpeg", "webp", "gif", "ogg", "mp3", "mp4", "webm", "7z", "gz",
    "bz2", "xz", "zst", "woff", "woff2", "tga", "dds", "bundled",
];

fn stored_for(rel: &str) -> bool {
    match rel.rsplit_once('.') {
        Some((_, ext)) => STORED_EXTS.contains(&ext.to_ascii_lowercase().as_str()),
        None => false,
    }
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
    let generated = write_root_files(input)?;
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
        start_jar: input
            .server_jar_name
            .clone()
            .or_else(|| input.installer_jar_name.clone()),
    })
}

/// 逐条目写入：jar 这类已压缩内容走 Stored（ deflate 再压一遍不省体积只烧时间），
/// 文本走 Deflated；每个顶层目录写完发一条 Group 事件
fn write_zip(
    zip: &mut zip::ZipWriter<BufWriter<File>>,
    plan: &[Planned],
    on_event: &mut dyn FnMut(&BuildEvent),
) -> Result<(), BuilderError> {
    let mut groups = group_totals(plan);
    for p in plan {
        let mut opts = SimpleFileOptions::default().compression_method(if stored_for(&p.rel) {
            CompressionMethod::Stored
        } else {
            CompressionMethod::Deflated
        });
        // zip 默认不携带 unix 权限（解压后 644），shell 脚本需补执行位
        if p.exec {
            opts = opts.unix_permissions(0o755);
        }
        zip.start_file(&p.rel, opts)?;
        let mut f = File::open(&p.path)?;
        std::io::copy(&mut f, zip)?;
        on_event(&BuildEvent::File { group: p.group.clone(), bytes: p.size });
        let Some(g) = groups.iter_mut().find(|g| g.label == p.group) else {
            continue;
        };
        g.done += 1;
        if g.done == g.files && !g.flushed {
            g.flushed = true;
            on_event(&BuildEvent::Group {
                label: g.label.clone(),
                files: g.files,
                bytes: g.bytes,
            });
        }
    }
    Ok(())
}

/// 待写入条目：绝对路径 + zip 内相对路径 + 大小 + 归属顶层分组
struct Planned {
    path: PathBuf,
    rel: String,
    size: u64,
    group: String,
    /// shell 脚本：zip 里要带 755 执行位
    exec: bool,
}

#[derive(Clone)]
struct GroupAcc {
    label: String,
    files: usize,
    bytes: u64,
    done: usize,
    flushed: bool,
}

/// staging 第一层目录即一个分组（mods/ 说「模组」，包根散件说「根文件」）——
/// 与取件阶段的分目录日志同一套口径
fn group_of(rel: &str) -> String {
    match rel.split_once('/') {
        Some((head, _)) if head.eq_ignore_ascii_case("mods") => "模组".to_string(),
        Some((head, _)) => head.to_string(),
        None => "根文件".to_string(),
    }
}

fn group_totals(plan: &[Planned]) -> Vec<GroupAcc> {
    let mut out: Vec<GroupAcc> = Vec::new();
    for p in plan {
        match out.iter_mut().find(|g| g.label == p.group) {
            Some(g) => {
                g.files += 1;
                g.bytes += p.size;
            }
            None => out.push(GroupAcc {
                label: p.group.clone(),
                files: 1,
                bytes: p.size,
                done: 0,
                flushed: false,
            }),
        }
    }
    out
}

/// 递归收集待打包条目（排序保证目录顺序稳定，日志与产物可复现）
fn collect(root: &Path, dir: &Path, out: &mut Vec<Planned>) -> Result<(), BuilderError> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)?.filter_map(|e| e.ok().map(|e| e.path())).collect();
    entries.sort();
    for path in entries {
        let meta = std::fs::metadata(&path)?;
        if meta.is_dir() {
            collect(root, &path, out)?;
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .map_err(|e| BuilderError::Io(std::io::Error::other(e)))?
            .to_string_lossy()
            .replace('\\', "/");
        out.push(Planned {
            group: group_of(&rel),
            exec: rel.to_lowercase().ends_with(".sh"),
            size: meta.len(),
            path,
            rel,
        });
    }
    Ok(())
}

/// Aikar's flags：官方推荐的 G1GC 调优参数组（4G+ 内存口径）
const AIKAR_FLAGS: &str = "-XX:+UseG1GC -XX:+ParallelRefProcEnabled -XX:MaxGCPauseMillis=200 \
-XX:+UnlockExperimentalVMOptions -XX:+DisableExplicitGC -XX:+AlwaysPreTouch \
-XX:G1NewSizePercent=30 -XX:G1MaxNewSizePercent=40 -XX:G1HeapRegionSize=8M \
-XX:G1ReservePercent=20 -XX:G1HeapWastePercent=5 -XX:G1MixedGCCountTarget=4 \
-XX:InitiatingHeapOccupancyPercent=15 -XX:G1MixedGCLiveThresholdPercent=90 \
-XX:G1RSetUpdatingPauseTimePercent=5 -XX:SurvivorRatio=32 -XX:+PerfDisableSharedMem \
-XX:MaxTenuringThreshold=1";

/// 内存 + Aikar + 用户附加参数 → 一行 JVM 参数（换行剔除，防注入第二行命令）
fn jvm_args(options: &ConversionOptions) -> String {
    let mut parts = vec![format!("-Xmx{}M", options.memory_mb)];
    if options.use_aikar_flags {
        parts.push(AIKAR_FLAGS.to_string());
    }
    let extra = options.extra_jvm_args.trim();
    if !extra.is_empty() {
        parts.push(extra.replace(['\r', '\n'], " "));
    }
    parts.join(" ")
}

/// 单行属性值清洗：换行→空格，反斜杠转义（server.properties 的 \n 语义）
fn one_line(s: &str) -> String {
    s.replace('\\', "\\\\").replace(['\r', '\n'], " ")
}

/// 启动脚本、eula、server.properties、README；返回本次实际生成的包根文件名
fn write_root_files(input: &BuildInput) -> Result<Vec<String>, BuilderError> {
    let mut generated: Vec<String> = Vec::new();
    let jvm = jvm_args(input.options);
    if input.options.generate_scripts {
        // 脚本先锚定自身目录：从任意 cwd 调用（终端/计划任务）相对 jar 路径仍然有效
        let (bat, sh) = match input.loader {
            LoaderKind::Fabric => {
                let jar = input.server_jar_name.as_deref().unwrap_or("server.jar");
                let nogui = if input.options.nogui { " nogui" } else { "" };
                (
                    format!(
                        "@echo off\r\ncd /d \"%~dp0\"\r\njava {jvm} -jar {jar}{nogui}\r\npause\r\n"
                    ),
                    format!("#!/usr/bin/env bash\ncd \"$(dirname \"$0\")\"\njava {jvm} -jar {jar}{nogui}\n"),
                )
            }
            LoaderKind::Forge | LoaderKind::NeoForge => {
                // JVM 参数经 user_jvm_args.txt 注入（installer 生成的 run 脚本以 @user_jvm_args.txt 引用）
                std::fs::write(input.staging.join("user_jvm_args.txt"), format!("{jvm}\n"))?;
                generated.push("user_jvm_args.txt".to_string());
                let installer = input
                    .installer_jar_name
                    .as_deref()
                    .unwrap_or("installer.jar");
                // 首次运行自动执行 installServer，之后用 installer 生成的 run 脚本启动
                (
                    format!(
                        "@echo off\r\ncd /d \"%~dp0\"\r\nif not exist run.bat java -Xmx1G -jar {installer} --installServer\r\nif exist run.bat (call run.bat) else (echo 安装失败，请手动运行: java -jar {installer} --installServer & pause)\r\n"
                    ),
                    format!(
                        "#!/usr/bin/env bash\ncd \"$(dirname \"$0\")\"\n[ -f run.sh ] || java -Xmx1G -jar {installer} --installServer\nbash run.sh\n"
                    ),
                )
            }
        };
        std::fs::write(input.staging.join("start.bat"), bat)?;
        std::fs::write(input.staging.join("start.sh"), sh)?;
        generated.push("start.bat".to_string());
        generated.push("start.sh".to_string());
    }
    // eula.txt 恒生成：开关只决定值（false 时服务端拒启，用户按 README 手改 true）
    std::fs::write(
        input.staging.join("eula.txt"),
        format!(
            "# eula=true 表示同意 Mojang 服务端最终用户协议（由 SideShift 按开关写入）\neula={}\n",
            if input.options.agree_eula { "true" } else { "false" }
        ),
    )?;
    generated.push("eula.txt".to_string());
    if !input.staging.join("server.properties").exists() {
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
        // UI 开关/下拉驱动的高频字段
        props.push_str(&format!("online-mode={}\n", o.online_mode));
        props.push_str(&format!("server-port={}\n", o.server_port));
        props.push_str(&format!("motd={}\n", one_line(&o.motd)));
        props.push_str(&format!("max-players={}\n", o.max_players));
        props.push_str(&format!("gamemode={gamemode}\n"));
        props.push_str(&format!("difficulty={difficulty}\n"));
        if !o.level_seed.trim().is_empty() {
            props.push_str(&format!("level-seed={}\n", one_line(o.level_seed.trim())));
        }
        // 其余按 vanilla 常用默认值给全（缺失键服务端首启也会自动补齐，这里给的是可读的完整模板）
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
level-type=default
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
        std::fs::write(
            input.staging.join("README-SideShift.txt"),
            input.readme_lines.join("\n") + "\n",
        )?;
        generated.push("README-SideShift.txt".to_string());
    }
    Ok(generated)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
