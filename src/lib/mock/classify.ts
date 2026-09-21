/**
 * 分类与取证的浏览器 dev 兜底：口径与 Rust 侧（detector / downloader）保持一致，
 * 只保证 UI 流程可走查，不含真实网络与 jar 解析。
 */
import type {
    AddedModSide,
    ConversionOptions,
    DownloadEstimate,
    PlanClassification,
    PlanMod,
} from "@/lib/types";

/** 下载量预估（浏览器 dev 兜底）：行聚合 + 加载器经验值；真实缓存扣减/HEAD 实测只在 Rust 侧 */
export async function mockEstimateDownload(
    plan: PlanMod[],
    options: ConversionOptions
): Promise<DownloadEstimate> {
    await new Promise((r) => setTimeout(r, 120));
    let downloadBytes = 0;
    let fromPackBytes = 0;
    for (const m of plan) {
        if (m.disposition === "remove") continue;
        if (m.needsDownload) downloadBytes += m.sizeBytes ?? 0;
        else fromPackBytes += m.sizeBytes ?? 0;
    }
    downloadBytes += options.loaderVersion ? (options.mcVersion === "1.20.1" ? 12_000_000 : 25_000_000) : 0;
    return { downloadBytes, fromPackBytes, complete: true };
}

/** 自动分类（浏览器 dev 兜底）：按与 Rust 侧同一套裁决口径重算处置，证据字段原样保留 */
export async function mockClassify(plan: PlanMod[]): Promise<PlanClassification> {
    await new Promise((r) => setTimeout(r, 200));
    const rows = plan.map((m) => {
        // 新增行与「需人工确认」行是样例设计意图（跨版本组件），不参与重算
        if (m.disposition === "add" || m.needsReview) return m;
        const { clientSide: c, serverSide: s } = m;
        const strip =
            s === "unsupported" ||
            (c === "required" && (s === "optional" || s === undefined));
        const source = m.envSource ?? "unknown";
        // 判不出两端 → 进剔除分组等人工确认；但 jar 内有服务端注册的行（名称层被字节码
        // 按住的那批）算有依据的保留，不落进待确认（与 detector 的 undecidable 同一条）
        if (source === "unknown" && m.bytecodeHint !== "serverCode") {
            return { ...m, disposition: "remove" as const, clientOnly: false, needsReview: true };
        }
        return {
            ...m,
            disposition: strip && source !== "unknown" ? ("remove" as const) : ("keep" as const),
            clientOnly: strip && source !== "unknown",
            // 服务端轴没答上 → 标待确认（分组仍由裁决决定）；jar 内确有服务端注册的行算有依据的保留
            needsReview: s === undefined && m.bytecodeHint !== "serverCode",
            envSource: source,
        };
    });
    // 浏览器预览没有联网层，一次到位
    return { plan: rows, onlinePending: false };
}

/**
 * 本地 jar 取证（浏览器 dev 兜底）：按文件名演示四种结局——
 * 纯客户端（误下载的典型）、服务端专属、两端必需（要提醒玩家客户端同装），
 * 以及中文改名/陌生包三层取证全空。
 */
export function mockInspectAdded(path: string): AddedModSide {
    const name = (path.split(/[\\/]/).pop() ?? path).toLowerCase();
    const sizeBytes = 320_000 + (name.length % 7) * 210_000;
    if (/(sodium|iris|voxelmap|moreculling|litematica|minihud)/.test(name)) {
        return { clientSide: "required", serverSide: "unsupported", envSource: "modrinthHash", sizeBytes };
    }
    if (/(spark|krypton|c2me|ferrite|server)/.test(name)) {
        return { clientSide: "unsupported", serverSide: "required", envSource: "jarMetadata", sizeBytes };
    }
    // 两端都必需：装进服务端包还不够，玩家客户端也得装同一个模组
    if (/(jei|create|botania|architectury)/.test(name)) {
        return { clientSide: "required", serverSide: "required", envSource: "modrinthHash", sizeBytes };
    }
    if (/(journeymap|xaero|fabric-api)/.test(name)) {
        return { clientSide: "optional", serverSide: "required", envSource: "modrinthHash", sizeBytes };
    }
    return { envSource: "unknown", sizeBytes };
}
