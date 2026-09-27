//! 卸载壳不需要内嵌任何载荷：真卸载器（`uninstall.exe`）此刻就躺在同一个安装目录里，
//! 壳只是调它。所以本文件只做图标那件事——和安装壳同一个坑：tauri-build 不盯图标文件，
//! 不显式声明就会链到旧的 resource.lib，改了图标 exe 却没变。

fn main() {
    println!("cargo:rerun-if-changed=icons/icon.ico");
    tauri_build::build();
}
