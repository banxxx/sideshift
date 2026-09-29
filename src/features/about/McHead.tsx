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
 *
 * 点击那一下的**自转**也归这里管（`spin`）：头像的 yaw 有两个写者（指针偏转与自转），
 * 谁都能写就必然互相拽，所以闸门、曲线与那条 rAF 全收在这一个模块里，`AckCard` 只负责按下去。
 */
import { forwardRef, useEffect, useImperativeHandle, useRef, useState, type ReactNode } from "react";
import { loadSkin, type SkinImage } from "./skin";
import { bakeKey, readHead, writeHead } from "./headCache";
import { createHead, headCanvasSize, headPose, voxelSupported, HEAD_BOX, type HeadHandle } from "./voxel";

/**
 * 自转的参数快照取自 `.scratch/ack-spin-proto.html`（`prof=B turns=2 total=1400 ramp=0.04
 * brake=0.7 pitch=hold snap=0`）。
 *
 * `ramp`/`brake` 是**时间轴占比**，中间那一段等速：56ms 提到满速 → 364ms 满速（≈1000°/s，
 * 读起来是一片糊）→ 980ms 三次方衰减到停。刹车段比满速段长两倍多，「转得飞快 + 慢慢收住」
 * 的对比全靠这两个数给的。而满速那个数＝360×圈数×V÷总时长 ⇒ 圈数与总时长是一起定速度的：
 * 加圈数整段一起变快，拉长总时长则两头一起慢，都不改变「快转 → 缓停」的形状。
 */
const SPIN = { turns: 2, totalMs: 1400, ramp: 0.04, brake: 0.7 };

/** 位置曲线：u∈[0,1] → 走完的行程比例。它的导数＝角速度：起步线性升、中段等速、尾段三次方降到 0 */
function spinFrac(u: number): number {
    const w = Math.min(SPIN.brake, 0.9);
    const r = Math.min(SPIN.ramp, 0.999 - w);
    const V = 1 / (1 - r / 2 - (2 * w) / 3); // ∫速度 du=1 定出等速段的速度
    const x = Math.min(1, Math.max(0, u));
    if (x < r) return (V * x * x) / (2 * r);
    const A1 = (V * r) / 2;
    if (x < 1 - w) return A1 + V * (x - r);
    const s = (x - (1 - w)) / w;
    return A1 + V * (1 - w - r) + (V * w * (1 - (1 - s) ** 3)) / 3;
}

export interface McHeadHandle {
    /** 归一化的指针偏移（-0.5..0.5）；增益与静止朝向都收在 `voxel.ts` 里，调用方不必知道。
     *  自转期间只记账不落笔（见 `spin` 那条「两个写者」） */
    setTilt: (dx: number, dy: number) => void;
    /** 点这一下 ⇒ 这枚头水平自转 `SPIN.turns` 圈后缓慢停住。闸门在内部：还在转就再点＝没点
     *  （不与上一圈叠加、不重开）。这台机器转不了（没有 webgl2）或压根没有 3D 那一层（平面头像、
     *  首字块）就只有调用方那一下按下，与样片里「静态图档转不了」那一档同口径 */
    spin: () => void;
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
        /** 正在自转：既是「偏转靠边站」的闸门，也是「再点吞掉」的闸门 */
        const spinning = useRef(false);
        const spinRaf = useRef(0);
        /** 自转期间收到的最后一份指针偏转——只记账不落笔，转完照它回位 */
        const lastTilt = useRef({ x: 0, y: 0 });

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

        // 卸载（切页、换语言重挂）时收掉那条还没转完的 rAF
        useEffect(() => () => cancelAnimationFrame(spinRaf.current), []);

        useImperativeHandle(
            ref,
            () => ({
                setTilt(dx, dy) {
                    lastTilt.current = { x: dx, y: dy };
                    // 碰过才建真的那一份：第一次偏转与点击同样是「碰过」，只有这一次值得进 state
                    if (!liveRef.current && (dx !== 0 || dy !== 0)) {
                        liveRef.current = true;
                        setLive(true);
                    }
                    // 自转期间不落笔，只记账：同一格 yaw 两个写者会互相拽，读起来就是「转不动」
                    if (spinning.current) return;
                    head.current?.setPose(...headPose(dx, dy));
                },
                spin() {
                    // 闸门（样片那一档 `lock=swallow`）：还在转就再点＝没点。不排队、不叠圈——
                    // 两圈抢同一个 yaw 比这一下没反应更难读
                    if (!supported || spinning.current) return;
                    if (!liveRef.current) {
                        liveRef.current = true;
                        setLive(true);
                    }
                    const [y0, p0] = headPose(lastTilt.current.x, lastTilt.current.y);
                    // `snap=0`：停在「起点 + 整圈数」，不吸回整圈——起点那一格偏转是指针给的，
                    // 留着它才读得出「从你点的那个朝向转出去」。pitch 全程不动（`pitch=hold`）：
                    // 绕水平轴翻一圈会露下巴，"展示"就变成"摔跤"
                    const deg = 360 * SPIN.turns;
                    const t0 = performance.now();
                    spinning.current = true;
                    const tick = (now: number) => {
                        const u = (now - t0) / SPIN.totalMs;
                        // 头还没建好（第一次进页、皮肤还在解）这几帧就是空的；建好那一帧接着当前
                        // 角度续上，所以不需要「攒一次待播」——真到了那一步也只是晚半圈
                        head.current?.setPose(y0 + deg * spinFrac(u), p0);
                        if (u < 1) {
                            spinRaf.current = requestAnimationFrame(tick);
                            return;
                        }
                        spinRaf.current = 0;
                        spinning.current = false;
                        // 收尾照最后记下的那份偏转回位：转的过程中指针动过的话，不留「卡片歪着、
                        // 头像还朝着中途」那一格——那是自转期间欠下的账，只有这里能还
                        head.current?.setPose(...headPose(lastTilt.current.x, lastTilt.current.y));
                    };
                    spinRaf.current = requestAnimationFrame(tick);
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
