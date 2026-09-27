/**
 * 一枚 3D 皮肤头像（体素档，`voxel.ts`），三层回落里最上面那一层。
 *
 * 为什么它自己管状态、不由 `AckCard` 判：皮肤是异步的（解一张 64×64 的贴图 + 建一次几何），
 * 拿不到之前那块地方不能空着——所以**没就绪就原样渲染传进来的那一层**（自带头像或名字首字），
 * 就绪了才换成画布。换的那一下只有透明度以外的属性都不变（同一个槽位、同一个尺寸），
 * 不重排、不重播入场。
 *
 * ## 三件事按「贵不贵」排开
 *
 *  1. **先问本机有没有烘好的那张**（`headCache`）。有就直接显示那张静止姿态的 PNG，
 *     一次皮肤都不解、一个 GL 槽位都不建——打开关于页本来只需要这个。
 *  2. **指针真的偏转过这枚头，才建真的那一枚**（`live`）。所以第一次进页（还没有烘档）
 *     仍然走实时那一趟，顺便把档烘出来；第二次起就只是显示一张图。
 *     切过去的像素与烘的那张是同一份（同一个画布、同一个边长），读起来不是"换了张图"。
 *  3. **本机那份都没有 ⇒ 立刻实时建**（`baked` 为空 ⇒ 不经过问）。这台机器没有 webgl2 的话
 *     `supported` 就已经是假，压根不读不写，永远停在传进来的那一层。
 *
 * 两条与卡片 3D 悬停的接口：
 *  - 偏转走 `setTilt` 这条**命令式**的线，不走 state：指针每动一次就 setState 会让整张卡重挂，
 *    而 `translateZ` 那层浮起是 CSS 的事，两者各转各的（同一个元素两条 transform 会互相顶掉）。
 *    只有「从没偏转过」到「偏转过」那一次例外，它才值得进一次 state。
 *  - 画布比槽位大一圈（头发凸出去的那截），所以它必须绝对定位、由槽位居中裁不出来——
 *    留白靠 `overflow` 不设限，卡片的 3D 层照旧吃 `translateZ`。烘出来那张走同一套几何，
 *    两者互换不动布局。
 */
import { forwardRef, useEffect, useImperativeHandle, useRef, useState, type ReactNode } from "react";
import { loadSkin, type SkinImage } from "./skin";
import { bakeKey, readHead, writeHead } from "./headCache";
import { createHead, headCanvasSize, headPose, voxelSupported, HEAD_BOX, type HeadHandle } from "./voxel";

export interface McHeadHandle {
    /** 归一化的指针偏移（-0.5..0.5）；增益与静止朝向都收在 `voxel.ts` 里，调用方不必知道 */
    setTilt: (dx: number, dy: number) => void;
}

export const McHead = forwardRef<McHeadHandle, { url: string; size: number; fallback: ReactNode }>(
    function McHead({ url, size, fallback }, ref) {
        const canvas = useRef<HTMLCanvasElement>(null);
        const head = useRef<HeadHandle | null>(null);
        const [skin, setSkin] = useState<SkinImage | null>(null);
        /** 本机那张烘好的图（object URL）。`bakedRef` 是同一个值给回调读的——回调节拍在画完之后，
         *  那时才知道「这次到底是不是白烘」（已经有档就别再写一遍同样的字节） */
        const [baked, setBaked] = useState<string | null>(null);
        const bakedRef = useRef<string | null>(null);
        /** 这枚头被指针碰过吗：碰过才值得建真的那一份（带偏转的那一套 GL 账） */
        const [live, setLive] = useState(false);
        const liveRef = useRef(false);

        // 这台机器没有 webgl2 就没必要去打 CDN：直接永远停在传进来的那一层
        const supported = voxelSupported();
        const key = bakeKey(url, headCanvasSize(size));

        useEffect(() => {
            if (!supported) return;
            let on = true;
            // 换皮肤＝换键，先把上一张的答复撤了：留着它会把新皮肤压成旧的那张脸
            setBaked(null);
            bakedRef.current = null;
            void readHead(key).then((u) => {
                if (!on || !u) return;
                bakedRef.current = u;
                setBaked(u);
            });
            return () => {
                on = false;
            };
        }, [supported, key]);

        useEffect(() => {
            if (!supported) return;
            // 有本机那张就等指针进来；没有（含这台机器烘不了）就立刻实时来一趟
            if (baked !== null && !live) return;
            let on = true;
            void loadSkin(url).then((s) => {
                if (on && s) setSkin(s);
            });
            return () => {
                on = false;
            };
        }, [supported, url, baked, live]);

        useEffect(() => {
            const el = canvas.current;
            if (!skin || !el) return;
            const h = createHead(el, skin, size, (cv) => {
                if (!key || bakedRef.current) return;
                cv.toBlob((b) => b && writeHead(key, b), "image/png");
            });
            if (!h) return;
            head.current = h.handle;
            return () => {
                head.current = null;
                h.destroy();
            };
        }, [skin, size, key]);

        useImperativeHandle(
            ref,
            () => ({
                setTilt(dx, dy) {
                    if (!liveRef.current && (dx !== 0 || dy !== 0)) {
                        liveRef.current = true;
                        setLive(true);
                    }
                    head.current?.setPose(...headPose(dx, dy));
                },
            }),
            []
        );

        // 画布与烘出来那张共用同一套几何：同一个槽位、同一圈外扩，谁换谁都不动布局
        const box = size * HEAD_BOX;
        const at = (size - box) / 2;
        const layer = "absolute block";
        const wrap = {
            inset: at,
            width: box,
            height: box,
        } as const;

        if (skin)
            return (
                <span
                    aria-hidden
                    className="relative shrink-0 transition-transform duration-200 group-hover/ack:[transform:translateZ(16px)]"
                    style={{ width: size, height: size }}
                >
                    <canvas ref={canvas} className={layer} style={wrap} />
                </span>
            );

        if (baked)
            return (
                <span
                    aria-hidden
                    className="relative shrink-0 transition-transform duration-200 group-hover/ack:[transform:translateZ(16px)]"
                    style={{ width: size, height: size }}
                >
                    <img className={layer} style={wrap} src={baked} alt="" draggable={false} />
                </span>
            );

        return <>{fallback}</>;
    }
);
