//! 把官方 NSIS 安装包内嵌进壳：发安装器时只给一个 exe，而不是"壳 + Setup.exe"两个文件。
//!
//! 顺序是先有鸡后有蛋，所以这条链必须由构建脚本走：**主应用出包 → 壳内嵌那个包**。
//! 找不到就硬失败并写明下一步该跑什么——静默用一个空壳去装出个 0 字节的"成功"，
//! 比编译不过难查得多。

use std::path::{Path, PathBuf};

/// 内嵌进二进制的安装包文件名（运行时原样落到 %TEMP% 再执行）
const PAYLOAD: &str = "setup-payload.bin";

fn main() {
    println!("cargo:rerun-if-env-changed=SIDESHIFT_RELEASE_DIR");

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
