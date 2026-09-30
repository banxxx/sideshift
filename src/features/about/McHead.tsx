/**
 * 一枚 3D 皮肤头像（体素档，`voxel.ts`），三层回落里最上面那一层；就绪前原样渲染传进来的 fallback。
 * 三档按贵不贵排：本机烘好的 PNG（`headCache`）→ 指针偏转过才建实时画布（`live`）→ 本机没有就立刻实时建；
 * 无 webgl2 则永远停在 fallback。换脸那一下只有透明度变，不重排、不重播入场。
 * 偏转走 `setTilt` 这条命令式的线（每指针事件 setState 会让整张卡重挂）；画布比槽位大一圈，须绝对定位由槽位裁。
 * 自转（`spin`）归本模块收口：头像 yaw 有两个写者（偏转与自转），闸门/曲线/收尾全在这里，调用方只负责按下去；
 * 转时偏转只记账不落笔，收尾朝向按指针此刻在不在命中区里分两档（偏移量分不出「正中心」与「已离开」）。
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
    /** 归一化的指针偏移（-0.5..0.5）与**指针当前在不在这张卡的命中区里**；增益与静止朝向都收在
     *  `voxel.ts` 里，调用方不必知道。自转期间只记账不落笔（见 `spin` 那条「两个写者」）。
     *
     * `inside` 不能省：**「指针在正中心」和「指针已经出去了」在偏移量上是同一个数 (0,0)**，
     * 只记偏移就没法分出自转结束时该朝哪（朝指针 / 朝正前）。
     */
    setTilt: (dx: number, dy: number, inside: boolean) => void;
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
        /** 指针此刻在不在这张卡的命中区里（偏移量本身分不出「正中心」与「已离开」） */
        const inside = useRef(false);

        /**
         * 自转结束时**该朝哪**（只用在收尾那一笔，过程中不走这里，见 `spin` 里那段为什么）：
         * 指针还在卡内就朝它当前的偏转（哪怕转的过程中手挪过），已经移出就朝正前方（0,0）。
         */
        const aimPose = (): [number, number] =>
            inside.current ? headPose(lastTilt.current.x, lastTilt.current.y) : [0, 0];

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
                setTilt(dx, dy, isInside) {
                    lastTilt.current = { x: dx, y: dy };
                    inside.current = isInside;
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
                    // 一圈的行程。起点那一格偏转是指针给的，留着它才读得出「从你点的那个朝向转出去」
                    const base = 360 * SPIN.turns;
                    const t0 = performance.now();
                    spinning.current = true;
                    const tick = (now: number) => {
                        const u = (now - t0) / SPIN.totalMs;
                        const f = spinFrac(u);
                        /* 终点每帧重算：把「该朝哪」折算进行程里，收尾就不是硬回位。
                         * `spinFrac(1)=1` ⇒ 最后一帧正好落在 `aim`（yaw 那 720° 是它的同角），
                         * 刹车段的尾巴于是**一路被指针牵着走**：手在卡内挪了，它跟着改方向收。
                         * 旧写法是转完照记到的偏再跳一下——指针不动才碰巧对，一动就差最多 30°
                         *（满偏 FINE_GAIN 30 的一半）。pitch 同一条式子：指针不动时 `ap=p0`，
                         * 仍是他定的那档「全程保持俯角」(`pitch=hold`)。
                         *
                         * 这里**只读缓冲过的那份偏转**（跟随器每帧指数逼近，进出都连续），不读
                         * `inside` 硬切：指针在刹车末段离开时 `f` 已经接近 1，硬切就是终点当场挪
                         * 最多 15° ⇒ 肉眼一下甩。分档判断只留在收尾那一笔。 */
                        const [ay, ap] = headPose(lastTilt.current.x, lastTilt.current.y);
                        // 头还没建好（第一次进页、皮肤还在解）这几帧就是空的；建好那一帧接着当前
                        // 角度续上，所以不需要「攒一次待播」——真到了那一步也只是晚半圈
                        head.current?.setPose(y0 + (base + ay - y0) * f, p0 + (ap - p0) * f);
                        if (u < 1) {
                            spinRaf.current = requestAnimationFrame(tick);
                            return;
                        }
                        spinRaf.current = 0;
                        spinning.current = false;
                        /* 收尾那一笔才分两档：指针还在这张卡的命中区里就朝它此刻的位置，
                         * 已经出去就朝正前（0,0）——顺带把跟随器缓冲没走完的那零点几度收干净。
                         * 落的是**原角**（不带那 720°）：slot 里存的数回到零区间，下一次自转的起点
                         * 与跟随器的写值同尺度，不会一路攒成大角度数。 */
                        head.current?.setPose(...aimPose());
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
