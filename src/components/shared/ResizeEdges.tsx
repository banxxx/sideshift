/**
 * 透明留白里的窗口缩放把手
 *
 * 无边框窗口的「拖边改尺寸」不归网页：tauri 在客户区上挂了一个原生子窗口
 * （tauri-runtime-wry `undecorated_resizing.rs`），它铺满整块 client 再抠掉中间，
 * 只留最外圈一条环，环宽 = `SM_CXFRAME/SM_CYFRAME`（随 DPI 同比放大，折成 CSS px 恒约 4px）。
 * 窗口透明 + 卡片内缩 `--win-inset`（12px）之后，那条环仍钉在**窗口**外沿 ⇒
 * 与卡片边缘之间留出 ~8px 死区，看得见摸不着，非得拖阴影外沿才反应。
 * 这里把整条留白接管：按下即 `startResizeDragging`，tao 侧是
 * `ReleaseCapture` + `PostMessage(WM_NCLBUTTONDOWN, HTLEFT…)`，与标题栏
 * `data-tauri-drag-region` 同一条路。环占的最外几 px 事件被原生子窗口吃掉，
 * 但行为与此处一致，两段拼成完整一条。
 *
 * 尺寸/光标全在 App.css 的 `.win-edge` 里：命中带向外到窗口边（`--win-inset`）、
 * 向内跨过卡片边（`--win-edge-inside`），卡片边落在带子中间才读得出「按在边框上」。
 * 两者在最大化时一起收成 0，这九个 div 自动缩成零宽（原生环那时也已被 tao 撤掉），
 * 所以这里不需要读最大化状态。
 */
import { getCurrentWindow, type Window } from "@tauri-apps/api/window";
import type { MouseEvent } from "react";
import { isTauri } from "@/lib/api";

/** 联合类型在 @tauri-apps/api 里只声明未导出，从方法签名上取，免得本地重列一份 */
type ResizeDirection = Parameters<Window["startResizeDragging"]>[0];

/** 四条边在前、四角在后：CSS 里同优先级靠源码顺序决胜，角才压得过边 */
const EDGES: ResizeDirection[] = [
    "West",
    "East",
    "North",
    "South",
    "NorthWest",
    "NorthEast",
    "SouthWest",
    "SouthEast",
];

export function ResizeEdges() {
    // 纯浏览器 dev 下没有窗口可拖，也不必挂这层透明命中区
    if (!isTauri) return null;

    const startResize =
        (direction: ResizeDirection) => (e: MouseEvent<HTMLDivElement>) => {
            if (e.button !== 0) return;
            // 不留文本选择的起手式；系统缩放循环随后接管指针
            e.preventDefault();
            getCurrentWindow()
                .startResizeDragging(direction)
                // 权限没配好 / 命令不可用时必须留下话，否则整层表现成「按了没反应」，
                // 而这正是我们要排查的那个故障本身
                .catch((err) => console.error("startResizeDragging 失败", err));
        };

    return (
        <>
            {EDGES.map((direction) => (
                <div
                    key={direction}
                    data-edge={direction.toLowerCase()}
                    className="win-edge"
                    onMouseDown={startResize(direction)}
                />
            ))}
        </>
    );
}
