/**
 * 透明留白里的窗口缩放把手：原生无边框缩放环只占窗口最外 ~4px，与卡片边缘之间有摸不着的死区；
 * 这里把整条留白接管，按下即 `startResizeDragging`，与原生环两段拼成完整一条。
 * 尺寸/光标全在 App.css 的 `.win-edge`（改这里先看那边）；最大化时带子收成 0、原生环也已被 tao 撤掉，所以不需要读最大化状态。
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
