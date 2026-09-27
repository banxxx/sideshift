/**
 * 鸣谢名单的 3D 皮肤头像：逐纹理像素一块有厚度的小面 + 深度缓冲 + 顶点色光照（体素档）。
 *
 * 三条硬约束是这套架构能成立的前提，改动前先看这里（数值口径全部取自
 * `.scratch/about-proto.html` 挑定的那一档，改这里就是改观感）：
 *
 *  1. **全应用只有一个 WebGL 上下文**：一张共享离屏画布渲一枚头，再 `drawImage` 拷进那张卡
 *     自己的 2D 画布 ⇒ 上下文数量恒为 1（浏览器攒到十几个就把最早的熔断，几十张卡各挂一个必炸）。
 *     卡片的 3D 悬停不吃亏：那张 2D 画布仍在 CSS 的 `preserve-3d` 层里，`translateZ` 照旧。
 *  2. **颜色全走顶点色、不采纹理** ⇒ 没有 NEAREST/边缘取样那一堆坑，而且透明 texel 干脆不出几何
 *     ——「透明处被染黑」那类黑块在这一档是结构上不可能，不是靠参数躲过去的。
 *  3. **只重画"脏了的那几张"**：指针偏转只有正在悬停的那张会动，静止的头像一次画完就不再进循环
 *     ⇒ 没有常驻 rAF，关于页挂满头像时也是空闲的（撞「日志量级=前端性能预算」那条）。
 *
 * 光照只算「轴对齐法向 × 灯向」，两盏灯的强度按参考实现给（Ambient 2 + Directional 1.2），
 * 漫反射同那条 1/π，并且亮度在**线性空间**上乘、出图再编码回 sRGB——这条是「像不像参考实现」
 * 的第一变量：直接在 sRGB 数值上直乘会整体发暗、中灰掉一档。
 *
 * 许可口径：两个参考仓库一行源码都不 copy（tr7zw Protective License 禁牟利、Axolotl 是 GPL-3），
 * 这里只照它公开的参数表（外扩系数、灯位、fov）把几何与光照自己重写一遍。
 */
import type { SkinImage } from "./skin";

/** 基础头半边长（模型单位）：一个纹理像素＝1，所以八格一面就是 8×8 个单位、居中在 ±4 */
const R0 = 4;
/** 外层（头饰）相对基础头的放大 ×100：125＝原版 `PlayerModel.head.compile(0.0625F)`，每边正好外扩 1 个纹理像素 */
const HS = 1.25;
/** 外层在原版那一格之外再多凸出去多少（模型单位）。0＝与外壳同一圈，这里给 0：凸出去的那一维留给 CSS 档对眼 */
const DEP = 0;
/** 透视 fov（度）。20° 那一档接近正交，是参考实现给的数 */
const FOV = 20;
/**
 * 画布相对头像的外扩：头发要往外凸出一截才有地方画。基础头正脸宽恒等于槽位 ⇒ 这一圈只是留边距。
 * 组件按它排布（画布比槽位大一圈、居中压出去），所以这一格必须与投影用同一个数。
 */
export const HEAD_BOX = 1.6;
/** 超采样倍率（真 dpr 之外再乘一档） */
const SUPERSAMPLE = 2;
/** 指针偏转增益（度/格，吃归一化后的 -0.5..0.5 偏移）；静止朝向是 0/0＝正对镜头。
 *  所以**实际满偏角是它的一半**（30 ⇒ ±15°），别把它当满偏读——`MAX_TILT` 才是。
 *  这个数直接决定「看不看得出是方的」：侧面与正脸的屏宽比≈tan(满偏)，±5° 只有 9%（读成一张平面图），
 *  ±15° 到 27%（三条棱都看得见）。代价是头像在槽位里略微变小，见 `UNITS_HALF`。 */
const FINE_GAIN = 30;
/** 指针能到的最坏偏转角（yaw、pitch 同时满格，那个角最占画布） */
const MAX_TILT = FINE_GAIN / 2;
/** 两盏灯（参考实现的 Ambient 2 + Directional 1.2）与灯位（方向灯 (2,4,3) 的方位/仰角） */
const AMBIENT = 2;
const DIRECTIONAL = 1.2;
const LIGHT_AZ = 35;
const LIGHT_EL = 50;
/** 一次 rAF 最多画几张：34 张同时首绘会挤出一个 100ms 的长帧，摊开五帧谁也看不出来 */
const PER_FRAME = 8;

/**
 * 头立方体的六个面：`t`＝皮肤 u 轴方向，`b`＝皮肤 v 轴方向，`n`＝外法向
 *（模型空间 x 右、y 上、z 朝镜头），`u/v`＝该面在 64×64 贴图上的区块左上角。
 * 条带四面 v 递增朝下（-y）、u 沿展开方向绕一圈（屏幕左面→前脸→屏幕右面→背面）；
 * 顶面 v 递增朝镜头（那一行与前脸共边）；底面 v 递增朝后——最后这一条两家 3D 实现读法相反，
 * 样片里给过旋钮，这里定死原版那一种（它不影响外观轮廓，只影响底面那 8 个 texel 的左右翻）。
 */
const HEAD_FACE = [
    { nm: "fa", t: [1, 0, 0], b: [0, -1, 0], n: [0, 0, 1], u: 8, v: 8 },
    { nm: "bk", t: [-1, 0, 0], b: [0, -1, 0], n: [0, 0, -1], u: 24, v: 8 },
    { nm: "ll", t: [0, 0, 1], b: [0, -1, 0], n: [-1, 0, 0], u: 0, v: 8 },
    { nm: "rl", t: [0, 0, -1], b: [0, -1, 0], n: [1, 0, 0], u: 16, v: 8 },
    { nm: "tp", t: [1, 0, 0], b: [0, 0, 1], n: [0, 1, 0], u: 8, v: 0 },
    { nm: "bt", t: [1, 0, 0], b: [0, 0, -1], n: [0, -1, 0], u: 16, v: 0 },
] as const;

const V3 = {
    cross: (a: readonly number[], b: readonly number[]) => [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ],
    dot: (a: readonly number[], b: readonly number[]) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2],
    /** 3×3 乘法（行主序）：CSS 的 `rotateX(rx) rotateY(ry)` 就是 Rx·Ry 这个次序 */
    mul: (A: number[][], B: number[][]) =>
        A.map((row) => B[0].map((_, c) => V3.dot(row, [B[0][c], B[1][c], B[2][c]]))),
};

/** yaw 与 CSS 同号（正＝前脸转向屏幕右）；pitch 必须取负才同号：CSS 的 y 轴朝下、这里朝上 */
function rotOf(yaw: number, pitch: number): number[][] {
    const a = (-pitch * Math.PI) / 180;
    const b = (yaw * Math.PI) / 180;
    const ca = Math.cos(a);
    const sa = Math.sin(a);
    const cb = Math.cos(b);
    const sb = Math.sin(b);
    return V3.mul(
        [
            [1, 0, 0],
            [0, ca, -sa],
            [0, sa, ca],
        ],
        [
            [cb, 0, sb],
            [0, 1, 0],
            [-sb, 0, cb],
        ]
    );
}

function projMat(fov: number, near: number, far: number): number[][] {
    const t = Math.tan((fov * Math.PI) / 360);
    const f = 1 / t;
    const d = far - near;
    return [
        [f, 0, 0, 0],
        [0, f, 0, 0],
        [0, 0, -(far + near) / d, (-2 * far * near) / d],
        [0, 0, -1, 0],
    ];
}

function m4mul(A: number[][], B: number[][]): number[][] {
    const r: number[][] = [];
    for (let i = 0; i < 4; i++) {
        r.push([]);
        for (let c = 0; c < 4; c++)
            r[i][c] = A[i][0] * B[0][c] + A[i][1] * B[1][c] + A[i][2] * B[2][c] + A[i][3] * B[3][c];
    }
    return r;
}

/** 3×3 → mat3 uniform（列主序展开） */
function m3flat(M: number[][]): Float32Array {
    return new Float32Array([
        M[0][0], M[1][0], M[2][0],
        M[0][1], M[1][1], M[2][1],
        M[0][2], M[1][2], M[2][2],
    ]);
}

/** 4×4 行主序 → WebGL 要的列主序 Float32Array */
function m4flat(M: number[][]): Float32Array {
    const out = new Float32Array(16);
    for (let i = 0; i < 4; i++) for (let c = 0; c < 4; c++) out[c * 4 + i] = M[i][c];
    return out;
}

const VS = `#version 300 es
in vec3 aPos; in vec3 aNrm; in vec4 aCol;
uniform mat3 uRot; uniform mat4 uVP;
out vec3 vN; out vec4 vC;
void main(){ vN = uRot * aNrm; vC = aCol; gl_Position = uVP * vec4(uRot * aPos, 1.0); }`;

const FS = `#version 300 es
precision highp float;
in vec3 vN; in vec4 vC;
uniform float uAmb, uDir; uniform vec3 uL;
out vec4 o;
/* 参考实现（three）用的是这条分段式 sRGB 传递函数，不是 pow(x,2.2) 的近似 ⇒ 照它写 */
vec3 toLin(vec3 s){ return mix(s / 12.92, pow((s + 0.055) / 1.055, vec3(2.4)), step(vec3(0.04045), s)); }
vec3 toSrgb(vec3 l){ return mix(l * 12.92, 1.055 * pow(l, vec3(1.0 / 2.4)) - 0.055, step(vec3(0.0031308), l)); }
const float RECIP_PI = 0.31830989;
void main(){
  /* 阈值与参考实现同口径：皮肤里有半透明像素就按 0.5/255 切（那些格子要留下来混合）。
   * alpha 只从贴图来，绝不在这一步染黑任何东西。 */
  if (vC.a < 0.5 / 255.0) discard;
  vec3 n = normalize(vN);
  /* MeshStandardMaterial 的漫反射自带 1/π ⇒ 两盏灯按参考实现的强度给，才不用另配一个补偿系数 */
  float f = (uAmb + uDir * max(0.0, dot(n, normalize(uL)))) * RECIP_PI;
  /* 亮度在线性空间上乘、出图编码回 sRGB（=three 的 outputColorSpace 那一趟） */
  o = vec4(toSrgb(toLin(vC.rgb) * f) * vC.a, vC.a);
}`;

interface Geom {
    vao: WebGLVertexArrayObject;
    buf: WebGLBuffer;
    /** 内层的顶点数：外层要叠在之上，所以分两次画（先内后外，透明混合才对） */
    base: number;
    total: number;
}

interface Slot {
    ctx: CanvasRenderingContext2D;
    n: number;
    skin: SkinImage;
    geom: Geom | null;
    yaw: number;
    pitch: number;
    dirty: boolean;
}

/** 全应用一份的 WebGL 上下文。`tried` 之后不再重试：拿不到就是拿不到，重试只会每帧抛异常 */
const GL: {
    tried: boolean;
    ok: boolean;
    gl: WebGL2RenderingContext | null;
    cv: HTMLCanvasElement | null;
    prog: WebGLProgram | null;
    u: Record<string, WebGLUniformLocation | null>;
} = { tried: false, ok: false, gl: null, cv: null, prog: null, u: {} };

const slots = new Set<Slot>();
let frame = 0;

function shader(g: WebGL2RenderingContext, type: number, src: string): WebGLShader {
    const s = g.createShader(type);
    if (!s) throw new Error("shader 创建失败");
    g.shaderSource(s, src);
    g.compileShader(s);
    if (!g.getShaderParameter(s, g.COMPILE_STATUS))
        throw new Error(g.getShaderInfoLog(s) || "shader 编译失败");
    return s;
}

/** 懒初始化：没有任何正版头像时，一个 WebGL 上下文都不会建 */
function ensure(): boolean {
    if (GL.tried) return GL.ok;
    GL.tried = true;
    try {
        const cv = document.createElement("canvas");
        const g = cv.getContext("webgl2", {
            alpha: true,
            antialias: true,
            premultipliedAlpha: true,
            preserveDrawingBuffer: true,
            depth: true,
            powerPreference: "low-power",
        });
        if (!g) throw new Error("拿不到 webgl2 上下文");
        const p = g.createProgram();
        if (!p) throw new Error("program 创建失败");
        g.attachShader(p, shader(g, g.VERTEX_SHADER, VS));
        g.attachShader(p, shader(g, g.FRAGMENT_SHADER, FS));
        g.linkProgram(p);
        if (!g.getProgramParameter(p, g.LINK_STATUS))
            throw new Error(g.getProgramInfoLog(p) || "program 链接失败");
        g.useProgram(p);
        GL.u = {
            rot: g.getUniformLocation(p, "uRot"),
            vp: g.getUniformLocation(p, "uVP"),
            amb: g.getUniformLocation(p, "uAmb"),
            dir: g.getUniformLocation(p, "uDir"),
            l: g.getUniformLocation(p, "uL"),
        };
        g.enable(g.DEPTH_TEST);
        g.enable(g.CULL_FACE);
        g.blendFuncSeparate(g.ONE, g.ONE_MINUS_SRC_ALPHA, g.ONE, g.ONE_MINUS_SRC_ALPHA);
        g.clearColor(0, 0, 0, 0);
        GL.cv = cv;
        GL.gl = g;
        GL.prog = p;
        GL.ok = true;
    } catch {
        GL.ok = false;
        GL.gl = null;
    }
    return GL.ok;
}

/* ---------- 皮肤 → 几何 ----------
 * 内层也逐纹理像素切格子（而不是整面一张 quad）只为了一个理由：alpha=0 的格子必须真空着，
 * 让下面的卡背透出来——与平面档「透明像素不画东西」同一口径。 */
function buildVoxels(d: ImageData, hat: boolean): { pos: number[]; nrm: number[]; col: number[]; base: number } {
    const pos: number[] = [];
    const nrm: number[] = [];
    const col: number[] = [];
    const R1 = R0 * HS;
    const put = (v: number[], nr: readonly number[], c: number[]) => {
        pos.push(v[0], v[1], v[2]);
        nrm.push(nr[0], nr[1], nr[2]);
        col.push(c[0], c[1], c[2], c[3]);
    };
    const sub = (a: number[], b: number[]) => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    const quad = (q: number[][], nv: readonly number[], c: number[]) => {
        /* 绕序自己判：四个角点连成的四边形，与给定法向反了就倒序 ⇒ 关掉背面剔除也不会翻车 */
        const s = V3.dot(V3.cross(sub(q[1], q[0]), sub(q[2], q[0])), nv) >= 0 ? 1 : -1;
        const a0 = s > 0 ? [0, 1, 2, 3] : [3, 2, 1, 0];
        put(q[a0[0]], nv, c);
        put(q[a0[1]], nv, c);
        put(q[a0[2]], nv, c);
        put(q[a0[0]], nv, c);
        put(q[a0[2]], nv, c);
        put(q[a0[3]], nv, c);
    };
    const tex = (u0: number, v0: number, i: number, j: number) => {
        if (i < 0 || i > 7 || j < 0 || j > 7) return 0;
        return d.data[((v0 + j) * 64 + (u0 + i)) * 4 + 3];
    };
    const rgba = (u0: number, v0: number, i: number, j: number) => {
        const p = ((v0 + j) * 64 + (u0 + i)) * 4;
        return [d.data[p] / 255, d.data[p + 1] / 255, d.data[p + 2] / 255, d.data[p + 3] / 255];
    };

    /** 一个面的标架：`P3(x,y,z)` 把「面内坐标」映到模型空间 */
    const frameOf = (f: (typeof HEAD_FACE)[number]) => {
        const ex = f.t;
        const ez = f.n;
        const ey = V3.cross(ez, ex);
        return {
            ex,
            ey,
            ez,
            P3: (x: number, y: number, z: number) => [
                ex[0] * x + ey[0] * y + ez[0] * z,
                ex[1] * x + ey[1] * y + ez[1] * z,
                ex[2] * x + ey[2] * y + ez[2] * z,
            ],
            /* v 递增在面内朝哪：顶/底两面的 b 轴与 -y 不同向，所以这一格是算出来的不是写死的 */
            vs: V3.dot(ey, f.b) >= 0 ? 1 : -1,
        };
    };

    HEAD_FACE.forEach((f) => {
        const { P3, ez, vs } = frameOf(f);
        const y0 = (j: number) => (vs > 0 ? j - 4 : 3 - j);
        for (let j = 0; j < 8; j++)
            for (let i = 0; i < 8; i++) {
                if (!tex(f.u, f.v, i, j)) continue;
                const c = rgba(f.u, f.v, i, j);
                const a = y0(j);
                quad([P3(i - 4, a, R0), P3(i - 3, a, R0), P3(i - 3, a + 1, R0), P3(i - 4, a + 1, R0)], ez, c);
            }
    });
    const neg = (a: readonly number[]) => [-a[0], -a[1], -a[2]];
    const base = pos.length / 3;
    if (!hat) return { pos, nrm, col, base };

    HEAD_FACE.forEach((f) => {
        const { ex, ey, ez, P3, vs } = frameOf(f);
        const u0 = f.u + 32;
        const v0 = f.v;
        const y0 = (j: number) => (vs > 0 ? j - 4 : 3 - j);
        /* 外层是一个**完整的立方体壳**：半边长 z1 = R0·放大 + 额外外扩，六块面板都贴到这个外径、
         * 平面方向也铺到 ±z1。曾经平面仍用 ±4（跟基础头同一格网）而面板在 z1 ⇒ 四条棱各缺一个
         * (z1−4) 的方口，补口的壁又落在 ±4 那一圈（比外壳内缩）⇒ 看上去「不是立方的、格子错位」。 */
        const z1 = R1 + DEP;
        const sc = z1 / 4;
        const X = (v: number) => v * sc;
        for (let j = 0; j < 8; j++)
            for (let i = 0; i < 8; i++) {
                if (!tex(u0, v0, i, j)) continue;
                const c = rgba(u0, v0, i, j);
                const y = X(y0(j));
                const x0 = X(i - 4);
                const x1 = X(i - 3);
                /* 外表面（这一格真正「看得见的那一面」）永远画 */
                quad([P3(x0, y, z1), P3(x1, y, z1), P3(x1, y + sc, z1), P3(x0, y + sc, z1)], ez, c);
                /* 四壁只在「隔壁那格没墨」时画：有墨就是内部面，画了是自作多情的共面 */
                if (!tex(u0, v0, i + 1, j))
                    quad([P3(x1, y, R0), P3(x1, y, z1), P3(x1, y + sc, z1), P3(x1, y + sc, R0)], ex, c);
                if (!tex(u0, v0, i - 1, j))
                    quad([P3(x0, y, z1), P3(x0, y, R0), P3(x0, y + sc, R0), P3(x0, y + sc, z1)], neg(ex), c);
                if (!tex(u0, v0, i, j + vs))
                    quad([P3(x0, y + sc, R0), P3(x1, y + sc, R0), P3(x1, y + sc, z1), P3(x0, y + sc, z1)], ey, c);
                if (!tex(u0, v0, i, j - vs))
                    quad([P3(x1, y, R0), P3(x0, y, R0), P3(x0, y, z1), P3(x1, y, z1)], neg(ey), c);
                /* 内端面在 R0——正压在基础面上，画了就是 z-fighting ⇒ 不画 */
            }
    });
    return { pos, nrm, col, base };
}

function upload(g: ReturnType<typeof buildVoxels>): Geom | null {
    const gl = GL.gl;
    const prog = GL.prog;
    if (!gl || !prog) return null;
    const n = g.pos.length / 3;
    /* pos/nrm/col 是「每顶点 3/3/4 个连续 float」的平铺数组 ⇒ 必须按 i*3、i*4 取，
     * 不能拿 g.pos[i] 当第 i 个顶点的第一维（那样相邻顶点会互相重叠，颜色读到位置的浮点数 ⇒ 一片白） */
    const inter = new Float32Array(n * 10);
    for (let i = 0; i < n; i++) {
        const o = i * 10;
        inter[o] = g.pos[i * 3];
        inter[o + 1] = g.pos[i * 3 + 1];
        inter[o + 2] = g.pos[i * 3 + 2];
        inter[o + 3] = g.nrm[i * 3];
        inter[o + 4] = g.nrm[i * 3 + 1];
        inter[o + 5] = g.nrm[i * 3 + 2];
        inter[o + 6] = g.col[i * 4];
        inter[o + 7] = g.col[i * 4 + 1];
        inter[o + 8] = g.col[i * 4 + 2];
        inter[o + 9] = g.col[i * 4 + 3];
    }
    const vao = gl.createVertexArray();
    const buf = gl.createBuffer();
    if (!vao || !buf) return null;
    gl.bindVertexArray(vao);
    gl.bindBuffer(gl.ARRAY_BUFFER, buf);
    gl.bufferData(gl.ARRAY_BUFFER, inter, gl.STATIC_DRAW);
    const stride = 40;
    ([["aPos", 3, 0], ["aNrm", 3, 12], ["aCol", 4, 24]] as const).forEach(([name, size, off]) => {
        const loc = gl.getAttribLocation(prog, name);
        gl.enableVertexAttribArray(loc);
        gl.vertexAttribPointer(loc, size, gl.FLOAT, false, stride, off);
    });
    gl.bindVertexArray(null);
    return { vao, buf, base: g.base, total: n };
}

/* ---------- 画布边长：几何在透视下不能贴着画布边 ----------
 * 投影把「unitsHalf 个模型单位」摊成半个画布，所以 unitsHalf 越大＝头在画布里越小。
 * 下限那条（`HEAD_BOX` 那一档）保证基础头正脸宽恒等于槽位；上限两条保证外层壳在最坏的**姿态包络**
 * 上也整个留在画布里。用包络而不是当帧角度：画布比例不能跟着指针抖（抖一下就是整张头像缩一下）。
 * 偏转角大到下限撑不住时，头会略微变小，而不是头发被切掉。 */
function worst(r: number, T: number, U: number, A: number): number {
    const ct = Math.cos(T);
    const st = Math.sin(T);
    const cu = Math.cos(U);
    const su = Math.sin(U);
    let m = 0;
    for (let s = 0; s < 8; s++) {
        const x = (s & 1 ? 1 : -1) * r;
        const y = (s & 2 ? 1 : -1) * r;
        const z = (s & 4 ? 1 : -1) * r;
        /* 先 yaw（绕 y）后 pitch（绕 x）——rotOf 就是这个序 */
        const xa = x * ct + z * st;
        const za = -x * st + z * ct;
        const ya = y * cu - za * su;
        const zu = y * su + za * cu;
        /* 先 pitch 后 yaw（画布是方的，横竖都得量；两序取大 ⇒ 不依赖组合顺序对不对） */
        const yb = y * cu - z * su;
        const zb = y * su + z * cu;
        const xb = x * ct + zb * st;
        const zv = -x * st + zb * ct;
        m = Math.max(m, Math.max(Math.abs(xa), Math.abs(ya)) + A * zu, Math.max(Math.abs(xb), Math.abs(yb)) + A * zv);
    }
    return m;
}

/** 与指针角度无关 ⇒ 整个模块算一次就够（静止朝向 0/0，包络就是 `MAX_TILT` 那一档） */
const UNITS_HALF = (() => {
    const A = Math.tan((FOV * Math.PI) / 360);
    const T = (MAX_TILT * Math.PI) / 180;
    const U = T;
    const z1 = R0 * HS + DEP;
    /* ×1.04：包络位上几何本来会精确贴到画布边（贴边＝抗锯齿那一圈读起来像被削平），留 4% 余量 */
    return Math.max(R0 * HEAD_BOX + R0 * A, 1.04 * worst(z1, T, U, A), 1.04 * worst(R0, T, U, A));
})();

/** 一帧：渲进共享离屏画布，再整张拷进这张卡自己的 2D 画布 */
function draw(s: Slot) {
    const gl = GL.gl;
    const cv = GL.cv;
    if (!gl || !cv) return;
    if (!s.geom) {
        s.geom = upload(buildVoxels(s.skin.opaque, s.skin.hat));
        /* 传不上去就是这台机器的 GL 不行了：留 dirty 会让每帧都重试一次，画不出来还白占一帧 */
        if (!s.geom) {
            s.dirty = false;
            return;
        }
    }
    const g = s.geom;
    if (cv.width !== s.n || cv.height !== s.n) {
        cv.width = s.n;
        cv.height = s.n;
    }
    gl.viewport(0, 0, s.n, s.n);
    gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
    const A = Math.tan((FOV * Math.PI) / 360);
    const Dc = UNITS_HALF / A;
    const near = Math.max(0.05, Dc - 14);
    const far = Dc + 14;
    const vp = m4mul(projMat(FOV, near, far), [
        [1, 0, 0, 0],
        [0, 1, 0, 0],
        [0, 0, 1, -Dc],
        [0, 0, 0, 1],
    ]);
    const az = (LIGHT_AZ * Math.PI) / 180;
    const el = (LIGHT_EL * Math.PI) / 180;
    gl.uniformMatrix3fv(GL.u.rot, false, m3flat(rotOf(s.yaw, s.pitch)));
    gl.uniformMatrix4fv(GL.u.vp, false, m4flat(vp));
    gl.uniform1f(GL.u.amb, AMBIENT);
    gl.uniform1f(GL.u.dir, DIRECTIONAL);
    gl.uniform3f(GL.u.l, Math.cos(el) * Math.sin(az), Math.sin(el), Math.cos(el) * Math.cos(az));
    gl.bindVertexArray(g.vao);
    /* 先内后外两趟：外层那些半透明的 texel 要按「身后已经画好的东西」去混合 */
    gl.drawArrays(gl.TRIANGLES, 0, g.base);
    if (s.skin.hat) gl.drawArrays(gl.TRIANGLES, 0, g.total);
    gl.bindVertexArray(null);
    s.ctx.clearRect(0, 0, s.n, s.n);
    s.ctx.drawImage(cv, 0, 0);
    s.dirty = false;
}

function flush() {
    frame = 0;
    let more = false;
    let drawn = 0;
    for (const s of slots) {
        if (!s.dirty) continue;
        if (drawn >= PER_FRAME) {
            more = true;
            continue;
        }
        draw(s);
        drawn++;
    }
    if (more) schedule();
}

function schedule() {
    if (!frame) frame = requestAnimationFrame(flush);
}

/** 体素档在这台机器上能不能用（没有 webgl2、或上下文创建失败 ⇒ 头像落回平面那张） */
export function voxelSupported(): boolean {
    return ensure();
}

export interface HeadHandle {
    /** 指针偏转（度）：yaw 跟 x、pitch 跟 y，两张卡各自独立，静止就是 0/0 */
    setPose: (yaw: number, pitch: number) => void;
}

/**
 * 归一化的指针偏移（-0.5..0.5）→ 头像自己的偏转角，与卡片的 CSS 倾角各走各的（同一元素两条 transform 会互相顶掉）。
 *
 * **符号口径以卡片为准**（`AckWall` 那句 `rotateX(-dy·2deg) rotateY(dx·2deg)`）：横轴同号、**纵轴取反**。
 * 这不是笔误而是两套坐标系差的一格：CSS 的 y 轴朝下、GL 的 y 轴朝上，同一个"指针在下"在两边
 * 要喂相反的角，头才会和卡片露出**同一侧**的面。反了的观感是卡片露顶、头像露下巴，
 * 两张皮各转各的＝"头像不跟鼠标走"。谁改那一侧的公式，这里就得跟着改。
 */
export function headPose(dx: number, dy: number): [number, number] {
    return [dx * FINE_GAIN, -dy * FINE_GAIN];
}
/**
 * 挂一枚头。返回句柄；`null`＝这台机器不支持体素档（调用方落回平面头像）。
 * 卸载必须调用 `destroy()`：VBO 挂在唯一那份上下文上，页面切来切去不回收就是每次都新攒一份。
 */
export function createHead(
    canvas: HTMLCanvasElement,
    skin: SkinImage,
    cssSize: number
): { handle: HeadHandle; destroy: () => void } | null {
    if (!ensure()) return null;
    const ctx = canvas.getContext("2d");
    if (!ctx) return null;
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    const n = Math.max(16, Math.round(cssSize * HEAD_BOX * dpr * SUPERSAMPLE));
    canvas.width = n;
    canvas.height = n;
    const slot: Slot = { ctx, n, skin, geom: null, yaw: 0, pitch: 0, dirty: true };
    slots.add(slot);
    schedule();
    return {
        handle: {
            setPose(yaw, pitch) {
                if (slot.yaw === yaw && slot.pitch === pitch) return;
                slot.yaw = yaw;
                slot.pitch = pitch;
                slot.dirty = true;
                schedule();
            },
        },
        destroy() {
            slots.delete(slot);
            const gl = GL.gl;
            if (gl && slot.geom) {
                gl.deleteVertexArray(slot.geom.vao);
                gl.deleteBuffer(slot.geom.buf);
            }
            slot.geom = null;
            if (!slots.size && frame) {
                cancelAnimationFrame(frame);
                frame = 0;
            }
        },
    };
}
