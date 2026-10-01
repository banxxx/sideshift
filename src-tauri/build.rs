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

fn main() {
    // tauri-build 只对 tauri.conf.json 和 capabilities 声明了 rerun-if-changed，图标不在其中：
    // 换掉 icons/icon.ico 后 build.rs 不会重跑，链接用的还是旧的 resource.lib，
    // 构建全程绿灯，任务栏/资源管理器里仍是上一个图标。实测踩过（1.0.0-beta.1 那版 exe 就是这样）。
    println!("cargo:rerun-if-changed=icons/icon.ico");
    // 同理，词典换了内容必须重压，否则 exe 里躺着的还是上一版的表
    println!("cargo:rerun-if-changed=assets/mcmod-names.tsv");
    pack_names_dict();
    tauri_build::build()
}
