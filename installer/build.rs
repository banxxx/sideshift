//! 把官方 NSIS 安装包与卸载壳 exe 内嵌进安装壳：对外只交一个 exe，不是「壳 + Setup.exe」两个文件。
//! 构建顺序固定且由本脚本校验：主应用出包 → 卸载壳出包 → 壳内嵌这两个包；
//! 找不到产物就硬失败并写明下一步该跑什么，绝不静默内嵌空载荷装出 0 字节的「成功」。

use std::path::{Path, PathBuf};

/// 内嵌进二进制的安装包文件名（运行时原样落到 %TEMP% 再执行）
const PAYLOAD: &str = "setup-payload.bin";
/// 内嵌的卸载壳 exe（运行时原样落到安装目录，NSIS 那一套删不到它，所以由壳自己投放）
const SHELL: &str = "uninstall-shell.bin";
/// 卸载壳产物的文件名，和它 `[[bin]] name` 一致
const SHELL_EXE: &str = "SideShift-Uninstall";

fn main() {
    println!("cargo:rerun-if-env-changed=SIDESHIFT_RELEASE_DIR");
    // 同主应用：tauri-build 不盯图标文件，不显式声明就会链到旧的 resource.lib
    println!("cargo:rerun-if-changed=icons/icon.ico");

    let dir = match std::env::var("SIDESHIFT_RELEASE_DIR") {
        Ok(v) if !v.trim().is_empty() => PathBuf::from(v),
        // 默认跟着主应用的 release 产物走：`tauri build` 出的 nsis 包和裸 exe 都在那儿
        _ => PathBuf::from("../src-tauri/target/release"),
    };
    let dir = std::path::absolute(&dir).unwrap_or(dir);

    let setup = find_setup(&dir).unwrap_or_else(|| {
        panic!(
            "找不到 NSIS 安装包（查过 {}）。先出主应用的包：pnpm tauri build --bundles nsis",
            dir.join("bundle").join("nsis").display()
        )
    });
    let bytes = std::fs::read(&setup).unwrap_or_else(|e| {
        panic!("读安装包 {} 失败：{e}", setup.display());
    });
    // 内嵌的是整个安装包的字节，尺寸一眼能看出有没有装错版本
    println!("cargo:warning=内嵌安装包 {}（{} 字节）", setup.display(), bytes.len());

    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    std::fs::write(out.join(PAYLOAD), &bytes).expect("写内嵌载荷失败");
    println!("cargo:rerun-if-changed={}", setup.display());

    // 卸载壳和主应用共用一个 target（依赖几乎重合，各留一棵树就是多占几 GB 磁盘）
    let shell = dir.join(format!("{SHELL_EXE}{}", std::env::consts::EXE_SUFFIX));
    let shell_bytes = std::fs::read(&shell).unwrap_or_else(|e| {
        panic!(
            "找不到卸载壳 {}（{e}）。先出它：pnpm uninstaller（或直接 pnpm installer，它会自己按顺序跑）"
            , shell.display()
        )
    });
    println!("cargo:warning=内嵌卸载壳 {}（{} 字节）", shell.display(), shell_bytes.len());
    std::fs::write(out.join(SHELL), &shell_bytes).expect("写卸载壳载荷失败");
    println!("cargo:rerun-if-changed={}", shell.display());

    // 进度条的分母：装完之后安装目录里那个主程序该有多大（拿不到时给 0，前端退化成限速爬升）
    let installed = dir.join(format!("SideShift{}", std::env::consts::EXE_SUFFIX));
    let size = std::fs::metadata(&installed).map(|m| m.len()).unwrap_or(0);
    println!("cargo:rustc-env=INSTALLED_EXE_BYTES={size}");

    tauri_build::build();
}

/// 在 `bundle/nsis` 里找安装包：文件名带版本号，构建时不猜死
fn find_setup(release_dir: &Path) -> Option<PathBuf> {
    let nsis = release_dir.join("bundle").join("nsis");
    let mut hits: Vec<PathBuf> = std::fs::read_dir(&nsis)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension().and_then(|x| x.to_str()) == Some("exe")
                && p.file_name()
                    .and_then(|x| x.to_str())
                    .is_some_and(|n| n.ends_with("-setup.exe"))
        })
        .collect();
    hits.sort();
    // 同目录下留着历史版本时取最新的（文件名以版本号开头，字典序即时间序）
    hits.pop()
}
