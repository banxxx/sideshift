//! 无边框窗口的系统投影（Windows）
//!
//! 口径：tauri.conf 里 `shadow: false`。tao 的 `shadow` 在 Windows 上不碰窗口样式
//! （`WS_THICKFRAME` 两种取值都在），只决定 `WM_NCCALCSIZE` 要不要把客户区往里缩一圈
//! 隐形边框厚度（Win10 上 左/右/下 ≈ 8px、上 = 0）。DWM 恰好把**投影和那条 1px 边框线
//! 画在同一圈带子里**：
//! - `shadow: true` → 让出带子 → 有投影，同时露出那条纯黑描边（上边没有，因为 top inset 恒 0）
//! - `shadow: false` → 内容铺满整个窗口矩形，把带子盖掉 → 线没了，投影也没了
//! 所以「保投影、去线」在配置层面无解。这里走另一条原生路径：给窗口**类**加
//! `CS_DROPSHADOW`，让 DWM 沿窗口矩形外侧补一层柔和投影——不占客户区、不动布局、不改样式。
//!
//! 已知代价：
//! - 类样式是共享的（本进程所有 tauri 窗口同一个类），当前应用只有一个主窗口，无影响
//! - 改类样式不影响 tao 每次 maximize/minimize 重写的 `GWL_STYLE`，因此不会被它抹掉

/// 给主窗口挂上 `CS_DROPSHADOW`；非 Windows 平台为空实现
pub fn attach(window: &tauri::WebviewWindow) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::HWND;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetClassLongPtrW, SetClassLongPtrW, SetWindowPos, CS_DROPSHADOW, GCL_STYLE, HWND_TOP,
            SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOZORDER, SWP_NOSIZE,
        };

        let Ok(handle) = window.hwnd() else {
            return;
        };
        // windows-sys 0.61 里 HWND 就是裸指针别名，tauri 返回的是 windows  crate 的同名 newtype
        let hwnd: HWND = handle.0 as _;
        unsafe {
            let style = GetClassLongPtrW(hwnd, GCL_STYLE) as u32;
            if style & CS_DROPSHADOW == 0 {
                SetClassLongPtrW(hwnd, GCL_STYLE, (style | CS_DROPSHADOW) as isize);
            }
            // 类样式改完要触发一次非客户区刷新，已存在的窗口才会重算投影
            let _ = SetWindowPos(
                hwnd,
                HWND_TOP,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            );
        }
    }
    #[cfg(not(windows))]
    let _ = window;
}
