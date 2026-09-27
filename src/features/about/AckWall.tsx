/**
 * 密排卡名单：卡片入场、悬浮 3D、点击波浪涟漪，以及「收起 ≤4 行 / 展开看全部」那一档高度。
 *
 * 五条硬约束都是撞出来的，改之前先读完：
 *
 *  1. **两条 transform 不能落在同一个元素上**。入场（motion 写 y/opacity）跑在外层 cell，
 *     倾斜（本组件用 inline style 写 transform）跑在内层按钮上：WAAPI 的 `fill:"both"` 会永久
 *     压住后来的 inline transform，两条挤一个节点上就是互相顶掉（样片里入场动画把视差压死过）。
 *     涟漪因此走 `scale` / `borderColor` 这两个**独立属性**——也正因为如此它**不能用 `filter`
 *     做"亮一下"**：`filter` 创建层叠上下文，会把 `preserve-3d` 当场压平。
 *     同一件事在退场那一拍也成立：跟随器与那条 WAAPI 复位都写 `transform` ⇒ 退场期由 `releasing`
 *     挡住跟随器，它只负责把头带回正对。
 *  2. **`will-change` 与子层的 `translateZ` 只在悬停那一张上挂**。常驻的话 N 张卡 = N 个合成层
 *     与 N 个 3D 上下文，撞「日志量级=前端性能预算」那条。
 *  3. **涟漪的半径参照是「看得见的这一档」（壳），不是整条名单**：默认只露 4 行时按全高算半径，
 *     会把大半时间花在屏外那些行上。半径取"被点这张 → 可见区四角"的最远一个，点哪儿都扫得到全片。
 *  4. 档高**量真 DOM**（按每张卡的 `offsetTop` 分行），不写公式：昵称字号跟卡尺走，
 *     公式算出来的行高会在换度量时差几像素。transform 不参与 offsetTop/offsetHeight，
 *     所以入场正演着也量得准。被裁住的行不演涟漪（名单很长时省掉一屏无效动画）。
 *  5. **可悬浮区靠一张透明壳外扩，上限是行距的一半**（`HIT = GAP / 2`）。卡的盒子只有 45px 高，
 *     行间空档 8px ⇒ 扫过去必断；倾斜又把卡的**可见**边缘推出布局盒 1~2px ⇒ 那 2px 上 hover
 *     来回闪。外扩走的是 padding 盒（命中判定包含祖先），所以它不改布局、不改档高、不改行数。
 *     再多给就是两张卡的命中区重叠，重叠区归后来者 ⇒ 读成「点在 A 上亮的却是 B」。
 *     命中区管：描边染色、投影、高光跟指针、3D 倾斜、"点它＝波源"；**不管**涟漪波及谁（那由半径说了算）。
 *
 * `overflow:hidden` 是常驻的，这里**不能**照公共 `Collapse` 的「演完撤 overflow」办：
 * 收起档的内容本来就比壳高，撤了当场漏出后面几行。所以卡片的 3D 翘起、投影与涟漪的放大
 * 全都被这个裁切框管着，留白见下面 `PAD`。
 *
 * 留白为什么挂在壳上而不是挤卡片的位：**裁切发生在 padding 盒**，于是「壳加 padding + 等值
 * 负 margin」两件事同时成立——裁切框外扩一圈，卡片的落位却一分不动（仍与「鸣谢名单」标题左对齐）。
 * 三个数各自的上限不一样，别图省事写成一个：
 *  - 左右 20：涟漪最大放大 18%，昵称顶到 150px 上限的那张卡（宽 ≈210）每边要吃 19px；
 *    再往外就顶到 Panel 的边框内缘（卡片的 p-5），圆角区会跟着被裁。
 *  - 上 10：正好等于 Panel 的 gap，再大就往标题那行压过去了。
 *  - 下 `GAP - HIT`：底部能给的档高 = 行距 − 下一行命中壳往上顶的那截。留白一旦 ≥ 行距就会
 *    漏出下一行卡的顶边（硬边线，比裁掉的软投影显眼得多）；而命中壳是透明的，它在可见档里
 *    还会**抢指针**（点上去没反应、被裁掉的卡反而被点亮）。所以这里只给 4px，配套地把
 *    悬浮投影的尾巴也收进 4px（见 `SHADOW`）——**「不漏下一行」与「投影不裁」两头，只能要一头。**
 */
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { PointerEvent as ReactPointerEvent } from "react";
import { motion, useReducedMotion } from "motion/react";
import { Btn, FoldBtn, Panel, PanelHead } from "@/components/ui";
import { COLLAPSE, waveFrontDurMs, waveFrontTime, WAVE_FRONT_EASE } from "@/lib/page-motion";
import { RISE } from "@/lib/springs";
import { useT } from "@/lib/i18n";
import type { AckPerson } from "@/lib/types";
import { cn } from "@/lib/utils";
import { McHead, type McHeadHandle } from "./McHead";
import { STEVE_URL as STEVE } from "./headCache";
import type { AckStatus } from "./useContributors";

/**
 * 首字兜底：连头像都没有时块上放的字符。
 *
 * 汉字取首字（「张三」→「张」），拉丁取词首字母最多两位（"Banxxx Ng"→"BN"）。
 * 拆词只用 ASCII 的空白与连字符类分隔符，别把全角空格当分隔符——那不是空白。
 */
function initialsOf(name: string): string {
    const s = name.trim();
    if (!s) return "?";
    const first = Array.from(s)[0];
    if (/[㐀-鿿]/.test(first)) return first;
    const words = s.split(/[\s._\-']+/, 3).filter(Boolean);
    if (words.length >= 2) return (words[0][0] + words[1][0]).toUpperCase();
    return s.slice(0, 2).toUpperCase();
}


/**
 * 默认头：`public/steve.png`（随安装包分发 ⇒ 不发请求，关于页「只有三种情形联网」那句仍然是真话）。
 *
 * 为什么这一档是 Steve 而不是一张占位图：游戏里没选皮肤的玩家显示的就是 Steve，所以这张脸是
 * 协议内的真实行为。它必须是**真皮肤布局**（64×64 / 64×32，`skin.ts:72` 的尺寸门只认这两档），
 * 而且喂的是最上面那层 3D（`McHead`），不是平面图。地址的单一出处在 `headCache.STEVE_URL`
 * ——烘档的键要认它，那边改了这个路径这里跟着走，别在这抄一份字面量。
 */


/** 以下数值全部取自 `.scratch/about-proto.html` 挑定的那一档（样片参数快照），改这里就是改观感 */
const S = 30;
/** 卡与卡的行列间距 */
const GAP = 8;
/** 收起档最多几行；行数不足则有几行显示几行 */
const MAX_ROWS = 4;
/** 可悬浮区向四个方向各外扩多少（＝行距的一半，再多与邻居重叠）；见文件头第 5 条 */
const HIT = GAP / 2;
/** 裁切框外扩的留白（左右 / 上 / 下），理由见文件头最后一段 */
const PAD = { x: 20, top: 10, bottom: GAP - HIT };
/** 悬浮投影的尾巴必须收进 `PAD.bottom`（4px）：样片那版 `0 10px 22px -8px` 要吃 13px，
 *  而"不漏下一行"只给得起 4px。值在 App.css 的 `--ack-shadow`（视觉资源与令牌同一处，
 *  组件只写 `hover:[box-shadow:var(--ack-shadow)]`——Tailwind 扫的是源码字面量，拼不出动态 class）。
 */
/** 悬浮：倾角上限（度）、凸起（px）、透视距离（px）、头像与昵称各自再浮的行程（px）、离开复位时长（ms） */
const TILT = { deg: 12, lift: 14, pd: 800, zAvatar: 16, zName: 16, backMs: 620 };
/** 跟随缓冲（ms）：姿态与目标之间每帧按真实时长指数逼近，`tau`＝贴上 63% 所需的毫秒数。
 *  70 ⇒ 一次扫过一整行约 200ms 内追上，读起来是"跟着手、但有重量"；填 0 附近就是回到过去的硬跳。
 *  `backTau` 是**头像**复位的同一量：指数衰减与卡那条刹车曲线同起点、同形状（头慢尾爬），
 *  580ms 内贴到 0.004 以内 ⇒ 与 `backMs: 620` 收尾对得上。 */
const FOLLOW = { tau: 70, backTau: 120 };
/** 贴上目标的判据（归一化偏移的一格）：到了就撤 rAF，静止悬停不留常驻循环 */
const SETTLED = 0.004;
/** 涟漪：强度（%）、波速（px/s，口径＝等速主体段的速度）、单张脉冲时长（ms）、点击锁（ms） */
const RIP = { strength: 18, speed: 250, durMs: 520, lockMs: 420 };
/** 入场：行程（px）与错峰（秒），错峰总时长压进 budget（否则 240 人要排到 2 秒开外） */
const ENTRY = { rise: 8, stagger: 0.008, budget: 0.7 };

/** `#rrggbb` → rgba：涟漪的描边透明度按距离连续变，而 `--accent` 在 App.css 里就是十六进制字面量 */
function withAlpha(hex: string, a: number): string {
    const h = hex.replace("#", "").trim();
    const full = h.length === 3 ? h.split("").map((c) => c + c).join("") : h;
    const n = parseInt(full, 16);
    if (Number.isNaN(n)) return hex;
    return `rgba(${(n >> 16) & 255},${(n >> 8) & 255},${n & 255},${a})`;
}

/**
 * 卡的 React key：名字，同名时再挂「这是第几次出现」。
 *
 * 不用「名字＋在整份名单里的位置」：那样中间插一个人，后面每张卡的 key 全变 ⇒ 整批重挂，
 * 已建好的头与 `failedSrc` 一起丢一遍、入场动画重演。同名那一档只看它自己那几个，插不进来的
 * 位置改动不动它。也不能只用名字——名单是他手编的 JSON，两个人重名是正常输入，撞 key 会让
 * React 只留一张。
 */
function cardKeys(people: AckPerson[]): string[] {
    const seen = new Map<string, number>();
    return people.map((p) => {
        const n = (seen.get(p.name) ?? 0) + 1;
        seen.set(p.name, n);
        return n === 1 ? p.name : `${p.name}#${n}`;
    });
}

/**
 * 一枚密排卡。头像那三层的落法见下面 `headUrl` 旁边那张表——**别照"回落链"三个字凭直觉改**，
 * 那三格各自是一条被点过名的决定。
 */
function AckCard({
    p,
    skin,
    skinReady,
    delay,
}: {
    p: AckPerson;
    skin?: string;
    /** 这批人的皮肤地址查完了没（没查完不能给默认头，否则每个有皮肤的人都先闪一张 Steve） */
    skinReady: boolean;
    delay: number;
}) {
    const btn = useRef<HTMLButtonElement>(null);
    /** 3D 头像的偏转走命令式：指针每动一次不该让整张卡重挂（见 `McHead` 的注释） */
    const head = useRef<McHeadHandle>(null);
    /** 正在演的那条复位动画：只握它自己的句柄——`getAnimations()` 一把 cancel 会把正扫过来的涟漪掐断 */
    const back = useRef<Animation | null>(null);
    const reduced = useReducedMotion();
    /** 记的是「哪一份 URL 失败了」而不是一个 bool：版本对账换了头像地址时它自己失效，
     *  不需要 effect 去复位（bool 会把这个人的新地址也一起压成首字块） */
    const [failedSrc, setFailedSrc] = useState<string | null>(null);

    /* ---- 指针 → 姿态的那条跟随器 -------------------------------------
     * 「指针在哪」与「现在画出去多少」分成两份：事件只改 `goal`，rAF 每帧把 `cur` 往
     * `goal` 按**真实时长**逼近（不是每帧固定比例——那样 120Hz 的缓冲毫秒数会只有 60Hz 的一半）。
     *
     * 三条输出（卡的 transform、跟指针的高光、3D 头像）都读同一份 `cur`，所以它们结构上不可能
     * 不同拍。以前是「同一个事件值写三次」，看着同步，其实是三处各自硬跳。
     */
    const cur = useRef({ x: 0, y: 0 });
    const goal = useRef({ x: 0, y: 0 });
    const raf = useRef(0);
    const stamp = useRef(0);
    /** 退场：卡片交给 `leave` 里那条 WAAPI 刹车曲线，跟随器这段时间只管把头像带回正对，不抢同一行 transform */
    const releasing = useRef(false);

    const paint = useCallback(() => {
        const { x, y } = cur.current;
        const el = btn.current;
        if (el && !releasing.current) {
            // 右移⇒右边往后（rotateY 取正）；下移同理取负（CSS 的 y 轴朝下）
            el.style.transform = `perspective(${TILT.pd}px) rotateX(${(-y * TILT.deg * 2).toFixed(2)}deg) rotateY(${(x * TILT.deg * 2).toFixed(2)}deg) translateZ(${TILT.lift}px)`;
            el.style.setProperty("--ack-gx", `${((x + 0.5) * 100).toFixed(1)}%`);
            el.style.setProperty("--ack-gy", `${((y + 0.5) * 100).toFixed(1)}%`);
        }
        head.current?.setTilt(x, y);
    }, []);

    const step = useCallback(() => {
        raf.current = 0;
        const c = cur.current;
        const g = goal.current;
        const now = performance.now();
        const dt = Math.min(64, now - stamp.current); // 标签页挂起回来那一下不该一帧冲完
        stamp.current = now;
        const a = 1 - Math.exp(-dt / (releasing.current ? FOLLOW.backTau : FOLLOW.tau));
        c.x += (g.x - c.x) * a;
        c.y += (g.y - c.y) * a;
        if (Math.abs(g.x - c.x) < SETTLED && Math.abs(g.y - c.y) < SETTLED) {
            c.x = g.x;
            c.y = g.y;
            paint();
            return;
        }
        paint();
        raf.current = requestAnimationFrame(step);
    }, [paint]);

    const chase = useCallback(() => {
        if (raf.current) return;
        stamp.current = performance.now();
        raf.current = requestAnimationFrame(step);
    }, [step]);

    /** 事件只负责换算目标：夹到 [-0.5, 0.5]（命中区外扩了 `HIT` 那一圈，指针会落在盒子外，不夹就倾过头） */
    const track = useCallback(
        (e: ReactPointerEvent<HTMLButtonElement>) => {
            const el = btn.current;
            if (!el || reduced) return;
            const r = el.getBoundingClientRect();
            goal.current = {
                x: Math.min(0.5, Math.max(-0.5, (e.clientX - r.left) / r.width - 0.5)),
                y: Math.min(0.5, Math.max(-0.5, (e.clientY - r.top) / r.height - 0.5)),
            };
            releasing.current = false;
            chase();
        },
        [chase, reduced]
    );

    const leave = useCallback(() => {
        // 头像跟着一起回正（卡片下面那条曲线不动它）
        releasing.current = true;
        goal.current = { x: 0, y: 0 };
        chase();
        const el = btn.current;
        if (!el) return;
        const from = el.style.transform;
        el.style.transform = "";
        back.current?.cancel();
        back.current = null;
        if (reduced || !from) return;
        // 复位走波浪第二段的刹车曲线：起始斜率 2.0 ⇒ 一撤就动，末端斜率 0.013 ⇒ 最后那点是爬回去的。
        // 单卡只有十几度/十几 px，不叠"等速主体段"——那一段是给横跨整片的大行程保匀速观感用的
        const a = el.animate([{ transform: from }, { transform: "none" }], {
            duration: TILT.backMs,
            easing: WAVE_FRONT_EASE,
            fill: "both",
        });
        back.current = a;
        a.onfinish = () => {
            if (back.current === a) back.current = null;
            // 必须撤：fill:"both" 留着会永久压住下一次的 inline transform
            a.cancel();
            el.style.transform = "";
        };
    }, [chase, reduced]);

    /**
     * 再指上来时先撤掉还在演的那条复位动画。它带 `fill:"both"`，优先级高于 inline transform ⇒
     * 不撤就是「离开一下，620ms 内这张卡对指针毫无反应」；扫过几行时几乎每张卡都撞上这条。
     * （样片正是在 `pointerenter` 里做的这一步，我第一版落生产时把它漏在了 `leave` 里。）
     *
     * `track(e)` 用**进入那一刻**的坐标起头：不这么做，第一帧的缓冲会从上一次停住的角度开始爬，
     * 而指针早就在别处了。
     */
    const enter = useCallback(
        (e: ReactPointerEvent<HTMLButtonElement>) => {
            back.current?.cancel();
            back.current = null;
            track(e);
        },
        [track]
    );

    // 卸载（切页、换语言重挂）时收掉没演完的那条动画与那条还没贴上目标的 rAF
    useEffect(
        () => () => {
            back.current?.cancel();
            if (raf.current) cancelAnimationFrame(raf.current);
        },
        []
    );

    /** 最下面那一层：名字首字块。没有 MC 身份、又没有可用自带图的人都落这里 */
    const initials = (
        <span
            aria-hidden
            className={cn(
                "flex shrink-0 items-center justify-center rounded-[4px] bg-surface-2",
                "font-mono text-[12px] font-semibold uppercase text-text-2",
                "transition-transform duration-200 group-hover/ack:[transform:translateZ(16px)]"
            )}
            style={{ width: S, height: S }}
        >
            {initialsOf(p.name)}
        </span>
    );

    /* 头像三层的落法，三条各自是被点过名的决定：
     *  - 有 `minecraftId`：正版皮肤 → 查完了却没有（离线 / 404 / 名字没查到）→ **3D Steve**。
     *    这一档整个不看 `avatar`：登记了 MC 身份的人以 MC 身份为准，两张皮并存等于各说一半。
     *  - 没有 `minecraftId`：`avatar` 平面一张 → 取不到（404、离线、图被删）就**首字块**，
     *    不落默认头：那人根本不是 MC 玩家，给一张 Steve 是替 TA 宣告了一个 TA 没宣告过的身份。
     *  - `skinReady` 为假（这批地址还在查）不给 Steve —— 否则每个**有**皮肤的人都会先闪一张 Steve。
     */
    const headUrl = skin ?? (p.minecraftId && skinReady ? STEVE : undefined);
    const flat =
        !p.minecraftId && p.avatar && failedSrc !== p.avatar ? (
            <img
                src={p.avatar}
                onError={() => setFailedSrc(p.avatar ?? null)}
                alt=""
                aria-hidden
                draggable={false}
                className="shrink-0 rounded-[4px] object-cover transition-transform duration-200 group-hover/ack:[transform:translateZ(16px)]"
                style={{ width: S, height: S }}
            />
        ) : (
            initials
        );

    return (
        <motion.div
            className="inline-flex min-w-0"
            initial={{ opacity: 0, y: ENTRY.rise }}
            animate={{ opacity: 1, y: 0, transition: { ...RISE, delay } }}
        >
            <button
                ref={btn}
                type="button"
                data-ack-card
                aria-label={p.name}
                onPointerEnter={enter}
                onPointerMove={track}
                onPointerLeave={leave}
                className={cn(
                    "group/ack relative flex min-w-0 items-center rounded-md border border-stroke bg-surface",
                    "transition-[border-color,box-shadow] duration-200 hover:border-(--ack-border)",
                    "hover:[box-shadow:var(--ack-shadow)]",
                    // 只在悬停那一张上开合成层
                    !reduced && "hover:will-change-transform"
                )}
                style={{
                    gap: S * 0.28,
                    padding: `${S * 0.22}px ${S * 0.42}px ${S * 0.22}px ${S * 0.22}px`,
                    transformStyle: "preserve-3d",
                }}
            >
                {/* 命中区外扩（透明，只吃指针不占布局）：祖先 :hover 与 pointer 事件都算在这张卡上。
                    相邻两张卡各扩 HIT ⇒ 在行的中线正好相遇，零重叠 */}
                <span aria-hidden className="absolute" style={{ inset: -HIT }} />
                {/* 三层回落的落法见上面 `headUrl` 旁边那张表。`flat` 同时是 `McHead` 的垫层：
                    贴图没解出来之前（第一次进页、离线）那块地方由它顶着，从不会空一下 */}
                {headUrl ? <McHead ref={head} url={headUrl} size={S} fallback={flat} /> : flat}
                <span className="max-w-[150px] truncate text-[12.5px] leading-[1.25] font-medium text-text-1 transition-transform duration-200 group-hover/ack:[transform:translateZ(16px)]">
                    {p.name}
                </span>
                {/* 高光跟指针走。这里不开 overflow:hidden——它会把 preserve-3d 的分层浮起压平 */}
                <span
                    aria-hidden
                    className="pointer-events-none absolute inset-0 rounded-[inherit] opacity-0 transition-opacity duration-200 group-hover/ack:opacity-100"
                    style={{
                        background:
                            "radial-gradient(78px 78px at var(--ack-gx,50%) var(--ack-gy,50%), rgba(255,255,255,.2), rgba(255,255,255,0) 62%)",
                    }}
                />
            </button>
        </motion.div>
    );
}

/**
 * 名单卡。取数的三档非 `ready` 状态各有各的实话：`pending` 还没定档、`none` 是远端在线但名单
 * 确实空着（刚发布就是这个样，给一句说明、不给重试钮），`failed` 是一次数据都没拿到过（重试出口在这）。
 */
export function AckWall({
    people,
    skins,
    skinReady,
    status,
    onRetry,
}: {
    people: AckPerson[];
    /** 玩家名 → 正版皮肤地址；没有这个人（没查、查不到、没网）就是表里少一行，卡片自己落回下一层 */
    skins: Record<string, string>;
    /** 这批皮肤地址查完了没（`useContributors` 的那一趟的落点）。没查完不给默认头，见 `AckCard` 的表 */
    skinReady: boolean;
    status: AckStatus;
    onRetry: () => void;
}) {
    const t = useT();
    const shell = useRef<HTMLDivElement>(null);
    const wall = useRef<HTMLDivElement>(null);
    const gate = useRef(0);
    const [open, setOpen] = useState(false);
    const [band, setBand] = useState({ closed: 0, full: 0, expandable: false });
    const reduced = useReducedMotion();

    /** 行按 `offsetTop` 分组：同一次换行落下来的卡 top 相同，差 1px 以内算测量误差 */
    const measure = useCallback(() => {
        const node = wall.current;
        if (!node) return;
        const rows: { top: number; h: number }[] = [];
        for (const cell of Array.from(node.children)) {
            const el = cell as HTMLElement;
            const last = rows[rows.length - 1];
            if (!last || Math.abs(last.top - el.offsetTop) > 1) {
                rows.push({ top: el.offsetTop, h: el.offsetHeight });
            } else {
                last.h = Math.max(last.h, el.offsetHeight);
            }
        }
        const k = Math.min(rows.length, MAX_ROWS);
        // 纯内容高：`PAD` 那份留白在下面的 `height` 里一次性加上（height 是 border-box，含 padding）
        const next = {
            closed: k ? rows[k - 1].top + rows[k - 1].h : 0,
            full: node.offsetHeight,
            expandable: rows.length > MAX_ROWS,
        };
        setBand((prev) =>
            prev.closed === next.closed && prev.full === next.full && prev.expandable === next.expandable
                ? prev
                : next
        );
    }, []);

    useLayoutEffect(measure, [measure, people]);
    // 窗口宽度一变行的分布就变（逢改必流体），档高与「有没有展开钮」都得跟着重算
    useEffect(() => {
        const node = wall.current;
        if (!node || typeof ResizeObserver === "undefined") return;
        const ro = new ResizeObserver(measure);
        ro.observe(node);
        return () => ro.disconnect();
    }, [measure]);

    // 人数或宽度变到"收起档就装得下"时退回收起态：否则符号停在朝上的那一面却没有可收的东西
    useEffect(() => {
        if (!band.expandable && open) setOpen(false);
    }, [band.expandable, open]);

    /** 点一张卡 ⇒ 波前从那儿扫过可见的这一档，半径内的邻居各脉冲一下 */
    const ripple = useCallback(
        (src: HTMLElement) => {
            if (reduced || !RIP.strength || !wall.current) return;
            // 前沿触发的闸门：锁窗口内的后续点击直接吞掉。不排队——两波浪叠在同一批卡上互相盖，
            // 比"这一下没反应"更难读
            const now = performance.now();
            if (now < gate.current) return;
            gate.current = now + RIP.lockMs;

            const o = src.getBoundingClientRect();
            const ox = o.left + o.width / 2;
            const oy = o.top + o.height / 2;
            const b = (shell.current ?? src).getBoundingClientRect();
            const R = Math.round(
                Math.max(
                    Math.hypot(ox - b.left, oy - b.top),
                    Math.hypot(b.right - ox, oy - b.top),
                    Math.hypot(ox - b.left, oy - b.bottom),
                    Math.hypot(b.right - ox, oy - b.bottom)
                )
            );
            const T = waveFrontDurMs(R, RIP.speed);
            const accent = getComputedStyle(document.documentElement).getPropertyValue("--accent");

            for (const el of Array.from(wall.current.querySelectorAll<HTMLElement>("[data-ack-card]"))) {
                if (el === src) continue;
                const r = el.getBoundingClientRect();
                // 被裁住的那几行不演：屏上看不见的东西没有观众。取严格比较——下一行的卡顶
                // 正好压在裁切线上（`PAD.bottom` 只给到行距减命中外扩），等于号会让它白演一次
                if (r.bottom <= b.top || r.top >= b.bottom) continue;
                const dx = r.left + r.width / 2 - ox;
                const dy = r.top + r.height / 2 - oy;
                const d = Math.hypot(dx, dy) || 1;
                if (d > R) continue;
                // 幅度按线性衰减（半径就是"这一圈以外不理会"）；曲线只管**什么时候**扫到
                const fall = 1 - d / R;
                const sc = (RIP.strength / 100) * (0.3 + 0.7 * fall);
                el.animate(
                    [
                        { scale: "1", borderColor: getComputedStyle(el).borderTopColor, easing: "cubic-bezier(.2,.8,.2,1)" },
                        {
                            scale: String(1 + sc),
                            borderColor: withAlpha(accent, 0.2 + 0.6 * fall),
                            offset: 0.34,
                            easing: WAVE_FRONT_EASE,
                        },
                        { scale: "1", borderColor: getComputedStyle(el).borderTopColor },
                    ],
                    { duration: RIP.durMs, delay: waveFrontTime(d / R) * T, fill: "none" }
                );
            }
            // 波源那张先被按下去一点、再慢慢回弹：没有圆环，这一下就是"点在这儿"的全部线索
            src.animate(
                [
                    { scale: "1", easing: "cubic-bezier(.2,.8,.2,1)" },
                    { scale: String(1 - (RIP.strength / 100) * 0.6), offset: 0.24, easing: WAVE_FRONT_EASE },
                    { scale: "1" },
                ],
                { duration: 300, fill: "none" }
            );
        },
        [reduced]
    );

    /** 档高 = 内容高 + 上下留白（`height` 是 border-box，padding 含在里面） */
    const height = (band.expandable && open ? band.full : band.closed) + PAD.top + PAD.bottom;
    const keys = useMemo(() => cardKeys(people), [people]);

    return (
        <Panel gap={10}>
            <PanelHead
                title={t("about.acknowledgements", "鸣谢名单")}
                right={
                    // 空态/待定档不挂展开钮：那一档没渲染名单，量不到行，`band` 是上一份数据的旧值
                    status === "ready" && band.expandable ? (
                        <FoldBtn
                            open={open}
                            label={
                                open
                                    ? t("about.collapse-ack", "收起鸣谢名单")
                                    : t("about.expand-ack", "展开鸣谢名单")
                            }
                            onClick={() => setOpen((v) => !v)}
                        />
                    ) : undefined
                }
            />
            {status !== "ready" ? (
                // 居中排：说明在上、重新获取在下。`py-6` 是给个头的档——不撑开就没有「垂直居中」可言
                <div className="flex flex-col items-center justify-center gap-2 rounded-md border border-stroke bg-surface px-3 py-6 text-center">
                    <span className="text-[12px] leading-[18px] text-text-3">
                        {status === "pending"
                            ? t("about.ack-pending", "正在获取鸣谢名单…")
                            : status === "none"
                              ? t("about.ack-none", "名单还没有登记的贡献者")
                              : t("about.ack-failed", "当前无法显示鸣谢名单")}
                    </span>
                    {/* 「名单空着」不放重试：按下去只会拿回同一份空表。这一句就是它的全部出口 */}
                    {status === "failed" && (
                        <Btn size="xs" onClick={onRetry}>
                            {t("about.ack-retry", "重新获取")}
                        </Btn>
                    )}
                </div>
            ) : (
                <motion.div
                    ref={shell}
                    initial={false}
                    animate={{ height }}
                    transition={COLLAPSE}
                    // 常驻裁剪，理由见文件头第 4 条；负 margin 抵掉 padding，卡片落位不动、裁切框外扩
                    className="overflow-hidden"
                    style={{
                        margin: `-${PAD.top}px -${PAD.x}px -${PAD.bottom}px`,
                        padding: `${PAD.top}px ${PAD.x}px ${PAD.bottom}px`,
                    }}
                    onClick={(e) => {
                        const card = (e.target as HTMLElement).closest("[data-ack-card]");
                        if (card) ripple(card as HTMLElement);
                    }}
                >
                    <div ref={wall} className="relative flex flex-wrap items-start" style={{ gap: GAP }}>
                        {people.map((p, i) => (
                            <AckCard
                                key={keys[i]}
                                p={p}
                                skin={p.minecraftId ? skins[p.name] : undefined}
                                skinReady={skinReady}
                                delay={i * Math.min(ENTRY.stagger, ENTRY.budget / Math.max(1, people.length))}
                            />
                        ))}
                    </div>
                </motion.div>
            )}
        </Panel>
    );
}
