/** 转换方案：剔除清单 / 下载量预估 / 自动分类（含分类事件订阅） */
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
    ConversionOptions,
    DownloadEstimate,
    PlanClassified,
    PlanClassification,
    PlanMod,
} from "@/lib/types";
import { EVENTS } from "@/lib/types";
import * as mock from "@/lib/mock";
import { invokeOrMock, isTauri } from "./client";

/** 剔除清单（Rust: list_excluded_mods） */
export async function listExcludedMods(): Promise<PlanMod[]> {
    if (!isTauri) return mock.mockExcludedMods;
    return invokeOrMock("list_excluded_mods", undefined, () => mock.mockExcludedMods);
}

/** 下载量预估（Rust: estimate_download(plan, options)）：与构建分类同源，含缓存扣减与 HEAD 实测 */
export async function estimateDownload(
    plan: PlanMod[],
    options: ConversionOptions
): Promise<DownloadEstimate> {
    if (!isTauri) return mock.mockEstimateDownload(plan, options);
    return invokeOrMock("estimate_download", { plan, options }, () =>
        mock.mockEstimateDownload(plan, options)
    );
}

/** 自动分类（Rust: classify_pack -> PlanClassification）：离线层立即返回，在线层随后走 onClassified 补全 */
export async function classifyPack(): Promise<PlanClassification> {
    if (!isTauri) return mock.mockClassify(mock.mockPlanMods);
    return invokeOrMock("classify_pack", undefined, () =>
        mock.mockClassify(mock.mockPlanMods)
    );
}

/** 订阅自动分类结果（离线先到、在线补全，两次事件同一 fileName） */
export function onClassified(
    cb: (e: PlanClassified) => void
): Promise<UnlistenFn> {
    if (!isTauri) return Promise.resolve(() => {});
    return listen<PlanClassified>(EVENTS.classified, (ev) => cb(ev.payload));
}
