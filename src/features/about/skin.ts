/**
 * 正版皮肤贴图 → 「立方体六面要采样哪 12 个 8×8 区块」那张取样图（64×64 的 RGBA）。
 * 只认原版两档尺寸（64×64 与旧 64×32），其余一律不认、当场落回平面头像。
 * Notch 透明度那一刀必须照抄原版：旧皮肤下半幅整片不透明 ⇒ 清空 (32,0)-(64,32)，于是没有头发外层；内层强制不透明同理。
 * 地址由后端给，字节前端直连 Mojang CDN（带 CORS 故画布不脏）；取到的字节交后端在配置目录存副本，`getImageData` 包 try 防 SecurityError。
 */
import * as api from "@/lib/api";

/** 一份皮肤的两种内层读法，加一条「这人有头饰吗」 */
export interface SkinImage {
    /** 游戏内口径：内层该透的地方就透着（卡背的颜色能从头发的缝里看见） */
    alpha: ImageData;
    /** 原版加载器口径：内层强制不透明。默认走这一档 */
    opaque: ImageData;
    /** 头饰那 8×8 区块里有没有墨：没有就不画外层（画了等于凭空多一圈壳） */
    hat: boolean;
}

/** 头饰外层在贴图上的位置（老档那半幅） */
const HEAD_HAT: Region = [32, 0, 64, 16];
/** Notch 那一刀的判定区：整片不透明 ⇒ 认定这是一张旧皮肤，于是清空老档的下半幅 */
const NOTCH_REGION: Region = [32, 0, 64, 32];
/** 内层强制不透明的三块：头、躯干、四肢（原版的另一刀，透明且 RGB=0 的点会被切成纯黑） */
const INNER_OPAQUE: Region[] = [
    [0, 0, 32, 16],
    [0, 16, 64, 32],
    [16, 48, 48, 64],
];

type Region = [number, number, number, number];

function alphaAt(d: ImageData, x: number, y: number): number {
    return d.data[(y * 64 + x) * 4 + 3];
}

function regionHasInk(d: ImageData, r: Region): boolean {
    for (let y = r[1]; y < r[3]; y++) for (let x = r[0]; x < r[2]; x++) if (alphaAt(d, x, y) > 0) return true;
    return false;
}

function makeInnerOpaque(d: ImageData): void {
    INNER_OPAQUE.forEach((r) => {
        for (let y = r[1]; y < r[3]; y++) for (let x = r[0]; x < r[2]; x++) d.data[(y * 64 + x) * 4 + 3] = 255;
    });
}

function notchHack(d: ImageData): boolean {
    const r = NOTCH_REGION;
    for (let y = r[1]; y < r[3]; y++) for (let x = r[0]; x < r[2]; x++) if (alphaAt(d, x, y) < 128) return false;
    for (let y = r[1]; y < r[3]; y++) for (let x = r[0]; x < r[2]; x++) d.data[(y * 64 + x) * 4 + 3] = 0;
    return true;
}

/**
 * 解码后的 `<img>` → 取样图。尺寸不对、画布脏、读不回像素，都回 null（调用方落回平面头像）。
 *
 * 1:1 画进 64×64 而不是缩放：旧皮肤占上 32 行，下 32 行留着空——那半幅头立方体永远不采样。
 * `imageSmoothingEnabled=false` 是必须的一条：插一下就把 8×8 的格子糊成一团，体素档逐 texel
 * 取色的前提当场没了。
 */
export function normalizeSkin(img: HTMLImageElement): SkinImage | null {
    const iw = img.naturalWidth;
    const ih = img.naturalHeight;
    if (iw !== 64 || (ih !== 64 && ih !== 32)) return null;
    const c = document.createElement("canvas");
    c.width = 64;
    c.height = 64;
    const g = c.getContext("2d", { willReadFrequently: true });
    let base: ImageData;
    try {
        if (!g) return null;
        g.imageSmoothingEnabled = false;
        g.clearRect(0, 0, 64, 64);
        g.drawImage(img, 0, 0, 64, ih, 0, 0, 64, ih);
        base = g.getImageData(0, 0, 64, 64);
    } catch {
        return null;
    }
    if (ih === 32) notchHack(base);
    const hat = regionHasInk(base, HEAD_HAT);
    const opaque = new ImageData(new Uint8ClampedArray(base.data), 64, 64);
    makeInnerOpaque(opaque);
    return { alpha: base, opaque, hat };
}

/** 同一张贴图只解一次：切页签、换语言重挂、展开收起都不该再打一次 CDN */
const cache = new Map<string, Promise<SkinImage | null>>();

/** 取一份皮肤。失败（离线、404、画布被脏、尺寸不对）一律回 null，不抛 */
export function loadSkin(url: string): Promise<SkinImage | null> {
    let p = cache.get(url);
    if (!p) {
        p = decode(url);
        cache.set(url, p);
    }
    return p;
}

function toBase64(buf: ArrayBuffer): string {
    const bytes = new Uint8Array(buf);
    let bin = "";
    // 分片再 btoa：一次 spread 整张字节数组会把调用栈撑爆（贴图只有 KB 级，但这条不该靠那个）
    for (let i = 0; i < bytes.length; i += 0x8000) {
        bin += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
    }
    return btoa(bin);
}

/**
 * 这张贴图的取数来源：本机那份副本命中就一个字节都不用出去；没命中才打 CDN，
 * 并把字节带上（解码成功后再交后端存，见 `decode`）。
 *
 * 为什么网络这一跳不搬进后端：只有浏览器吃这台机器的代理（后端 reqwest 不吃，实测口径），
 * 搬过去会把「现在显示得出来的人」变成「显示不出来」。后端只管存与取 ⇒ 这条缓存
 * 一次都不会让可达性变差，只会让它少一次。
 */
async function skinSource(url: string): Promise<{ src: string; persist: string | null }> {
    // 内置那份默认皮肤（`/steve.png`，随包分发）不在贴图 CDN 上，两趟 IPC 都是白问：
    // 后端那个缓存键要求地址带 `…/texture/<内容哈希>`，同源路径永远过不了门
    if (!url.startsWith("http")) return { src: url, persist: null };
    const local = await api.loadTextureCache(url);
    if (local) return { src: `data:image/png;base64,${local}`, persist: null };
    let resp: Response;
    try {
        resp = await fetch(url);
    } catch {
        return { src: url, persist: null };
    }
    // 只存 PNG：后端那个文件名就以 `.png` 为口径，不该由它做第二次嗅探。
    // 不是 PNG（或不是 2xx）就照常显示、只是这次不落盘
    if (!resp.ok || resp.headers.get("content-type")?.split(";")[0] !== "image/png") {
        return { src: url, persist: null };
    }
    const buf = await resp.arrayBuffer();
    const b64 = toBase64(buf);
    return { src: `data:image/png;base64,${b64}`, persist: b64 };
}

async function decode(url: string): Promise<SkinImage | null> {
    const { src, persist } = await skinSource(url);
    const img = new Image();
    img.crossOrigin = "anonymous";
    img.decoding = "async";
    const ok = await new Promise<boolean>((res) => {
        img.onload = () => res(true);
        img.onerror = () => res(false);
        img.src = src;
    });
    const skin = ok ? normalizeSkin(img) : null;
    // 解得成才存：尺寸不对/读不回像素的字节没有复用价值，存下去只是让下次还是解不出、
    // 却从此不再有机会拿到一份好的
    if (persist && skin) api.saveTextureCache(url, persist);
    return skin;
}
