/**
 * 3D 头像「静止那一帧」的渲染结果，存在本机。
 *
 * 为什么存**渲染产物**而不是把渲染本身调快：产物只有一份 PNG 字节要读；实时那一趟是
 * 解皮肤 → 逐格建体素 → 上传一份每人独占的 VBO → GL 画 → 再整张拷进这张卡的 2D 画布，
 * 全部发生在「打开关于页」那一拍上，人数越多越明显（`voxel.ts` 的 `PER_FRAME = 8` 就是给
 * 这一拍设的节流，观感是"一批批陆续显形"）。有了产物，进页一个 GL 槽位都不建，指针进卡才建真的。
 *
 * 键必须唯一标识「这张图长什么样」，所以是 **贴图内容 + 画布边长 + 渲染格式号**：
 *  - Mojang 的贴图地址末段本来就是内容的 SHA-1 ⇒ 换皮肤＝换地址＝换键，旧那份自然作废。
 *    这比挂名单 `version` 准：某个人换了皮肤不该等整份名单升版才生效。
 *  - 边长（`headCanvasSize`）吃的是尺寸 × dpr × 超采样 ⇒ 换显示器、改缩放自动是另一条记录。
 *  - `H_FMT` 是渲染口径的版本号：改几何、光照、超采样就 +1，整批作废重烘。
 * 只烘对得上形状的两种地址（正版贴图、内置的 Steve），对不上就不烘、照常实时渲染——
 * 键一旦不能唯一标识内容，缓存下来的就是一张会长期顶替别人脸的图。
 *
 * 为什么放 IndexedDB 而不是配置目录那三件（名单快照 / 贴图地址 / 皮肤字节）旁边：
 * 这是**派生件**，删了下次自己重烘，而那三件是源数据。放这边还顺带绕开了「文件字节 base64 过一趟
 * IPC」——一屏几百枚 × 几 KB，那趟搬运正是这次要省的东西。代价：WebView 数据被重置 ⇒ 缓存没了，
 * 下次进页重烘一趟，用户看不出差别；换皮肤留下的旧记录不剪枝（单张几 KB，键自带内容哈希，攒不快）。
 */

/** 渲染口径的版本号：`voxel.ts` 的几何/光照/超采样任一改动就 +1，整批作废重烘。
 *  点名两条最容易忘的：`FOV` 与 `MAX_TILT`（＝`FINE_GAIN/2`）都喂给 `UNITS_HALF`，
 *  改了它们**静止那一帧的头会变大变小**——静止帧看着没动，其实整套投影标定都跟着动了。 */
const H_FMT = "h1";
/** 内置的默认皮肤（正版皮肤取不到时那档）。跟 `public/steve.png` 一份，随安装包分发，不发请求 */
export const STEVE_URL = "/steve.png";
/** Mojang 贴图 CDN 的地址形状，末段就是皮肤内容的 SHA-1 */
const MOJANG_TEXTURE = /^https:\/\/textures\.minecraft\.net\/texture\/([0-9a-f]{64})$/;

const DB_NAME = "sideshift-heads";
const STORE = "heads";

/** 对不上形状 ⇒ `null`＝这条不烘（调用方照常实时渲染、不读也不写缓存） */
export function bakeKey(url: string, px: number): string | null {
    const content = MOJANG_TEXTURE.exec(url)?.[1] ?? (url === STEVE_URL ? "steve" : null);
    return content ? `${H_FMT}-${content}-${px}` : null;
}

let dbp: Promise<IDBDatabase | null> | null = null;

/** 打不开（没有 indexedDB、隐私模式、版本被更低的页面占着）一律回 `null`：缓存不该挡渲染 */
function db(): Promise<IDBDatabase | null> {
    if (!dbp) {
        dbp = new Promise((res) => {
            try {
                const req = indexedDB.open(DB_NAME, 1);
                req.onupgradeneeded = () => {
                    if (!req.result.objectStoreNames.contains(STORE)) req.result.createObjectStore(STORE);
                };
                req.onsuccess = () => res(req.result);
                req.onerror = () => res(null);
                req.onblocked = () => res(null);
            } catch {
                res(null);
            }
        });
    }
    return dbp;
}

const NOTHING: Promise<string | null> = Promise.resolve(null);

/**
 * 一个键本次启动只问一次，并且 `createObjectURL` 只造一次。
 * 名单里共用同一张皮肤（默认 Steve 那一大片）是常态，不去重就是几百次同字节的读。
 * 造的 URL 不撤销：条数上限就是「这批名单里有几种皮肤 × 一档画布尺寸」，页面活着还要用。
 */
const urls = new Map<string, Promise<string | null>>();

export function readHead(key: string | null): Promise<string | null> {
    if (!key) return NOTHING;
    let p = urls.get(key);
    if (!p) {
        p = get(key);
        urls.set(key, p);
    }
    return p;
}

function get(key: string): Promise<string | null> {
    return db().then((d) => {
        if (!d) return null;
        return new Promise<string | null>((res) => {
            try {
                const r = d.transaction(STORE, "readonly").objectStore(STORE).get(key);
                r.onsuccess = () => res(r.result ? URL.createObjectURL(r.result as Blob) : null);
                r.onerror = () => res(null);
            } catch {
                res(null);
            }
        });
    });
}

/** 回存。写不写得成都不管：这一次已经画出来了，下次再烘一遍的代价是几 KB 的一次 GL 渲染 */
export function writeHead(key: string | null, blob: Blob): void {
    if (!key) return;
    void db().then((d) => {
        if (!d) return;
        try {
            d.transaction(STORE, "readwrite").objectStore(STORE).put(blob, key);
        } catch {
            // 存储配额、事务期间连接被关：都不该冒到界面上
        }
    });
}
