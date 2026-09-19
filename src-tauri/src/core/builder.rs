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
    pub options: &'a ConversionOptions,
    pub loader: LoaderKind,
    /// Fabric：一体化服务端 jar 文件名；Forge/NeoForge 为 None（用 installer + run 脚本）
    pub server_jar_name: Option<String>,
    /// Forge/NeoForge installer jar 文件名
    pub installer_jar_name: Option<String>,
    /// 写入包根的说明文件名（可空）
    pub readme_lines: Vec<String>,
}

pub fn build(input: &BuildInput) -> Result<(PathBuf, u64), BuilderError> {
    write_root_files(input)?;
    std::fs::create_dir_all(input.output_dir)?;
    let out_path = input.output_dir.join(&input.output_file_name);
    let file = File::create(&out_path)?;
    let mut zip = zip::ZipWriter::new(BufWriter::new(file));
    let opts: SimpleFileOptions =
        SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    zip_dir(&mut zip, input.staging, input.staging, &opts)?;
    zip.finish()?;
    let size = out_path.metadata().map(|m| m.len()).unwrap_or(0);
    Ok((out_path, size))
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

/// 启动脚本、eula、server.properties、README
fn write_root_files(input: &BuildInput) -> Result<(), BuilderError> {
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
    }
    // eula.txt 恒生成：开关只决定值（false 时服务端拒启，用户按 README 手改 true）
    std::fs::write(
        input.staging.join("eula.txt"),
        format!(
            "# eula=true 表示同意 Mojang 服务端最终用户协议（由 SideShift 按开关写入）\neula={}\n",
            if input.options.agree_eula { "true" } else { "false" }
        ),
    )?;
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
    }
    if !input.readme_lines.is_empty() {
        std::fs::write(
            input.staging.join("README-SideShift.txt"),
            input.readme_lines.join("\n") + "\n",
        )?;
    }
    Ok(())
}

fn zip_dir(
    zip: &mut zip::ZipWriter<BufWriter<File>>,
    root: &Path,
    dir: &Path,
    opts: &SimpleFileOptions,
) -> Result<(), BuilderError> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        let rel = path
            .strip_prefix(root)
            .map_err(|e| BuilderError::Io(std::io::Error::other(e)))?
            .to_string_lossy()
            .replace('\\', "/");
        if path.is_dir() {
            zip_dir(zip, root, &path, opts)?;
        } else {
            // zip 默认不携带 unix 权限（解压后 644），shell 脚本需补执行位
            let file_opts = if rel.to_lowercase().ends_with(".sh") {
                opts.unix_permissions(0o755)
            } else {
                *opts
            };
            zip.start_file(&rel, file_opts)?;
            let mut f = File::open(&path)?;
            std::io::copy(&mut f, zip)?;
        }
    }
    Ok(())
}
