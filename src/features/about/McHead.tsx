/**
 * 一枚 3D 皮肤头像（体素档，`voxel.ts`），三层回落里最上面那一层。
 *
 * 为什么它自己管状态、不由 `AckCard` 判：皮肤是异步的（解一张 64×64 的贴图 + 建一次几何），
 * 拿不到之前那块地方不能空着——所以**没就绪就原样渲染传进来的那一层**（自带头像或名字首字），
 * 就绪了才换成画布。换的那一下只有透明度以外的属性都不变（同一个槽位、同一个尺寸），
 * 不重排、不重播入场。
 *
 * 两条与卡片 3D 悬停的接口：
 *  - 偏转走 `setTilt` 这条**命令式**的线，不走 state：指针每动一次就 setState 会让整张卡重挂，
 *    而 `translateZ` 那层浮起是 CSS 的事，两者各转各的（同一个元素两条 transform 会互相顶掉）。
 *  - 画布比槽位大一圈（头发凸出去的那截），所以它必须绝对定位、由槽位居中裁不出来——
 *    留白靠 `overflow` 不设限，卡片的 3D 层照旧吃 `translateZ`。
 */
import { forwardRef, useEffect, useImperativeHandle, useRef, useState, type ReactNode } from "react";
import { loadSkin, type SkinImage } from "./skin";
import { createHead, headPose, voxelSupported, HEAD_BOX, type HeadHandle } from "./voxel";

export interface McHeadHandle {
    /** 归一化的指针偏移（-0.5..0.5）；增益与静止朝向都收在 `voxel.ts` 里，调用方不必知道 */
    setTilt: (dx: number, dy: number) => void;
}

export const McHead = forwardRef<McHeadHandle, { url: string; size: number; fallback: ReactNode }>(
    function McHead({ url, size, fallback }, ref) {
        const canvas = useRef<HTMLCanvasElement>(null);
        const head = useRef<HeadHandle | null>(null);
        const [skin, setSkin] = useState<SkinImage | null>(null);

        // 这台机器没有 webgl2 就没必要去打 CDN：直接永远停在传进来的那一层
        const supported = voxelSupported();
        useEffect(() => {
            if (!supported) return;
            let on = true;
            void loadSkin(url).then((s) => {
                if (on && s) setSkin(s);
            });
            return () => {
                on = false;
            };
        }, [supported, url]);

        useEffect(() => {
            const el = canvas.current;
            if (!skin || !el) return;
            const h = createHead(el, skin, size);
            if (!h) return;
            head.current = h.handle;
            return () => {
                head.current = null;
                h.destroy();
            };
        }, [skin, size]);

        useImperativeHandle(
            ref,
            () => ({
                setTilt(dx, dy) {
                    head.current?.setPose(...headPose(dx, dy));
                },
            }),
            []
        );

        if (!skin) return <>{fallback}</>;
        const box = size * HEAD_BOX;
        return (
            <span
                aria-hidden
                className="relative shrink-0 transition-transform duration-200 group-hover/ack:[transform:translateZ(16px)]"
                style={{ width: size, height: size }}
            >
                <canvas
                    ref={canvas}
                    className="absolute block"
                    style={{ inset: (size - box) / 2, width: box, height: box }}
                />
            </span>
        );
    }
);
