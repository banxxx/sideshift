fn main() {
    // tauri-build 只对 tauri.conf.json 和 capabilities 声明了 rerun-if-changed，图标不在其中：
    // 换掉 icons/icon.ico 后 build.rs 不会重跑，链接用的还是旧的 resource.lib，
    // 构建全程绿灯，任务栏/资源管理器里仍是上一个图标。实测踩过（1.0.0-beta.1 那版 exe 就是这样）。
    println!("cargo:rerun-if-changed=icons/icon.ico");
    tauri_build::build()
}
