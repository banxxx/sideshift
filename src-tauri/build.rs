use flate2::write::GzEncoder;
use flate2::Compression;
use std::io::Write;
use std::path::Path;

/// 内置词典的压缩腿：TSV（560KB）→ gzip（约 240KB）写进 OUT_DIR，运行期 include_bytes! 解一次。
/// 走构建期压缩而不是把 blob 检进仓库：仓库里那份是可与人对账的文本表，二进制只存在于产物里。
fn pack_names_dict() {
    let src = Path::new("assets/mcmod-names.tsv");
    let raw = std::fs::read(src).expect("assets/mcmod-names.tsv 缺失：node scripts/gen-mcmod-names.mjs <词条快照> 重新生成");
    let mut enc = GzEncoder::new(Vec::new(), Compression::best());
    enc.write_all(&raw).expect("压缩词典失败");
    let blob = enc.finish().expect("收尾词典失败");
    let out = Path::new(&std::env::var("OUT_DIR").expect("构建期没有 OUT_DIR")).join("mcmod-names.gz");
    std::fs::write(&out, &blob).expect("写入词典 blob 失败");
}

/// 卸载壳的内嵌：它是「卸载壳自愈」的载荷（应用每次启动把卸载入口改回自绘壳，
/// 见 `core::uninstall_shell`）。与安装壳的 build.rs 同一条硬规则：
///
/// - **release 构建缺壳 = 硬失败**。悄悄嵌一份空载荷等于让「更新后卸载向导变回原生」
///   这个毛病在构建全绿的情况下悄悄回归，而且运行期毫无声音（release 是 windows 子系统
///   没有控制台、debug 又不跑自愈）——那一档必须在打包这一刻就炸出来。
/// - **debug 构建缺壳只警告**：全新 clone 直接 `cargo test` / `pnpm tauri dev` 不该被
///   发版链的前置步骤卡住，嵌一份空载荷、运行期据此安静跳过。
/// - **候选路径无条件注册监听**：哪怕这次没找到也逐个挂 `rerun-if-changed`——
///   只在找到时挂的话，卸载壳后来才出现，cargo 不会重跑这一段，空载荷就顺着
///   target 缓存一直带下去（CI 上尤甚）。
fn embed_uninstall_shell() {
    const NAME: &str = "SideShift-Uninstall.exe";
    let mut candidates = vec![Path::new("target/release").join(NAME)];
    if let Ok(dir) = std::env::var("SIDESHIFT_RELEASE_DIR") {
        candidates.push(Path::new(&dir).join(NAME));
    }
    if let Ok(dir) = std::env::var("CARGO_TARGET_DIR") {
        candidates.push(Path::new(&dir).join("release").join(NAME));
    }
    for p in &candidates {
        println!("cargo:rerun-if-changed={}", p.display());
    }
    println!("cargo:rerun-if-env-changed=SIDESHIFT_RELEASE_DIR");
    println!("cargo:rerun-if-env-changed=CARGO_TARGET_DIR");

    let found = candidates.iter().find(|p| p.is_file());
    let bytes = found.and_then(|p| std::fs::read(p).ok()).unwrap_or_default();
    match found {
        Some(p) => {
            println!("cargo:warning=内嵌卸载壳 {}（{} 字节）", p.display(), bytes.len());
        }
        None if std::env::var("PROFILE").as_deref() == Ok("release") => {
            panic!(
                "release 构建找不到卸载壳（查过 {} 个候选路径）。                 先出它：pnpm uninstaller——没有它，应用内更新之后自绘卸载向导会悄悄变回原生的",
                candidates.len()
            );
        }
        None => {
            println!(
                "cargo:warning=没找到卸载壳（先跑 pnpm uninstaller 才有卸载壳自愈）；                 debug 构建嵌空载荷、自愈运行期会安静跳过"
            );
        }
    }
    let out = Path::new(&std::env::var("OUT_DIR").expect("OUT_DIR")).join("uninstall-shell.bin");
    std::fs::write(&out, &bytes).expect("写卸载壳载荷失败");
}

fn main() {
    // tauri-build 只对 tauri.conf.json 和 capabilities 声明了 rerun-if-changed，图标不在其中：
    // 换掉 icons/icon.ico 后 build.rs 不会重跑，链接用的还是旧的 resource.lib，
    // 构建全程绿灯，任务栏/资源管理器里仍是上一个图标。实测踩过（1.0.0-beta.1 那版 exe 就是这样）。
    println!("cargo:rerun-if-changed=icons/icon.ico");
    // 同理，词典换了内容必须重压，否则 exe 里躺着的还是上一版的表
    println!("cargo:rerun-if-changed=assets/mcmod-names.tsv");
    pack_names_dict();
    embed_uninstall_shell();
    tauri_build::build()
}
