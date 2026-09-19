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
        let (bat, sh) = match input.loader {
            LoaderKind::Fabric => {
                let jar = input.server_jar_name.as_deref().unwrap_or("server.jar");
                let nogui = if input.options.nogui { " nogui" } else { "" };
                (
                    format!(
                        "@echo off\r\njava {jvm} -jar {jar}{nogui}\r\npause\r\n"
                    ),
                    format!("#!/usr/bin/env bash\njava {jvm} -jar {jar}{nogui}\n"),
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
                        "@echo off\r\nif not exist run.bat java -Xmx1G -jar {installer} --installServer\r\nif exist run.bat (call run.bat) else (echo 安装失败，请手动运行: java -jar {installer} --installServer & pause)\r\n"
                    ),
                    format!(
                        "#!/usr/bin/env bash\n[ -f run.sh ] || java -Xmx1G -jar {installer} --installServer\nbash run.sh\n"
                    ),
                )
            }
        };
        std::fs::write(input.staging.join("start.bat"), bat)?;
        std::fs::write(input.staging.join("start.sh"), sh)?;
    }
    if input.options.agree_eula {
        std::fs::write(
            input.staging.join("eula.txt"),
            "# 由 SideShift 按用户设置写入（eula=同意 Mojang 服务端最终用户协议）\neula=true\n",
        )?;
    }
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
        props.push_str(&format!("online-mode={}\n", o.online_mode));
        props.push_str(&format!("server-port={}\n", o.server_port));
        props.push_str(&format!("motd={}\n", one_line(&o.motd)));
        props.push_str(&format!("max-players={}\n", o.max_players));
        props.push_str(&format!("gamemode={gamemode}\n"));
        props.push_str(&format!("difficulty={difficulty}\n"));
        if !o.level_seed.trim().is_empty() {
            props.push_str(&format!("level-seed={}\n", one_line(o.level_seed.trim())));
        }
        props.push_str("view-distance=10\nsimulation-distance=10\n");
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
            zip.start_file(&rel, *opts)?;
            let mut f = File::open(&path)?;
            std::io::copy(&mut f, zip)?;
        }
    }
    Ok(())
}
