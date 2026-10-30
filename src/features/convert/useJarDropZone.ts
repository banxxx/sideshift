/**
 * 「模组方案」卡的 .jar 拖入区。
 * Tauri 下 HTML5 drop 拿不到真实路径，只能听 webview 级事件——而那是整窗的，所以命中与否得自己量。
 * 命中的下边界取卡底出口区的顶边而不是卡底：范围永远是「列表那一块」，行数多少都不撑大它，
 * 也不会把「从本地添加」那两颗按钮圈进来。
 */
import { useEffect, useRef, useState } from "react";
import type { DragEvent, RefObject } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { isTauri } from "@/lib/api";

/** 拖入区自己的矩形（CSS 像素）：底边让给卡底出口区 */
function zoneOf(area: HTMLElement | null, footer: HTMLElement | null) {
    if (!area) return null;
    const a = area.getBoundingClientRect();
    const f = footer?.getBoundingClientRect();
    const bottom = f ? Math.min(a.bottom, f.top) : a.bottom;
    return { left: a.left, right: a.right, top: a.top, bottom, height: Math.max(0, bottom - a.top) };
}

export function useJarDropZone(
    area: RefObject<HTMLElement | null>,
    footer: RefObject<HTMLElement | null>,
    enabled: boolean,
    onPaths: (paths: string[]) => void
) {
    const [over, setOver] = useState(false);
    // 虚线框的高度只能量出来：行区是 flex-1，写死一档会在依赖警告出现/收起时错开
    const [height, setHeight] = useState(0);
    const cb = useRef(onPaths);
    cb.current = onPaths;

    useEffect(() => {
        if (!isTauri || !enabled) {
            // 拖到一半这一档被关掉（正好开始自动分类、或切去回看态）：退订不等于撤框，
            // 不在这儿补一刀，虚线框就永久留在卡上了
            setOver(false);
            return;
        }
        let unlisten: (() => void) | undefined;
        let revoked = false;
        // payload 的 position 是物理像素，而矩形是 CSS 像素：不除 dpr 会在缩放档下偏
        const hits = (pos: { x: number; y: number }) => {
            const zone = zoneOf(area.current, footer.current);
            if (!zone) return false;
            const dpr = window.devicePixelRatio || 1;
            const x = pos.x / dpr;
            const y = pos.y / dpr;
            if (x < zone.left || x > zone.right || y < zone.top || y > zone.bottom) return false;
            setHeight(zone.height);
            return true;
        };
        getCurrentWebview()
            .onDragDropEvent((event) => {
                const p = event.payload;
                if (p.type === "drop") {
                    // 松手当场撤高亮：成功丢下之后 Windows 不会再发 leave，留着它就永久盖在列表上。
                    // 先算命中再关，两件事不能反——框外松手也要撤
                    const inside = hits(p.position);
                    setOver(false);
                    if (inside) cb.current(p.paths);
                    return;
                }
                if (p.type === "leave") {
                    setOver(false);
                    return;
                }
                // enter / over 只跟悬停态（over 不带 paths，所以悬停期间只说规则、不列文件名）
                setOver(hits(p.position));
            })
            .then((fn) => {
                // 订阅是异步的，而 StrictMode 会在订阅回来之前就先退一次：
                // 不在这里补一刀，那一次泄漏的监听会永久留着 ⇒ 一次松手被处理两遍
                if (revoked) fn();
                else unlisten = fn;
            });
        return () => {
            revoked = true;
            unlisten?.();
        };
    }, [enabled, area, footer]);

    // 浏览器 dev 下没有绝对路径，把文件名交给同一条落地链：虚线框、上限、批量形状都能先看一眼
    const domHandlers = isTauri
        ? {}
        : {
              onDragOver: (e: DragEvent) => {
                  e.preventDefault();
                  const zone = zoneOf(area.current, footer.current);
                  if (!zone) return;
                  setHeight(zone.height);
                  setOver(true);
              },
              onDragLeave: () => setOver(false),
              onDrop: (e: DragEvent) => {
                  e.preventDefault();
                  setOver(false);
                  const names = Array.from(e.dataTransfer.files).map((f) => f.name);
                  if (names.length > 0) cb.current(names);
              },
          };

    return { over, height, domHandlers };
}
