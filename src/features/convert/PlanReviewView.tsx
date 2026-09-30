/**
 * 转换方案只读视图：任务详情「方案」签的全部内容，数据源只有任务存档（`getTask` + `getTaskPlan`）。
 * 不能改跑 `defaultOptions`/`classifyPack`——那两个只认「最近一次解析的包」且解析缓存不落盘，重启后必空、中途换包则张冠李戴。
 * 控件一律灰化到「一眼看出点不动」（`READONLY_BOX`/`READONLY_MARK`），但值保持可读；不提供「照存档再转一次」（存档无 sha1 字节锚，源包被改过会静默产出对不上的包）。
 */
import { useEffect, useMemo, useState } from "react";
import * as api from "@/lib/api";
import { useT } from "@/lib/i18n";
import { loaderLabel, reviewFirst } from "@/lib/format";
import type {
    ConversionOptions,
    ModDisposition,
    PackDirTree,
    PackManifest,
    PlanMod,
} from "@/lib/types";
import { Panel, type SelectOption } from "@/components/ui";
import { PlanListModal, type ListFocus } from "@/features/convert/modals";
import { KeepDirsCard, LaunchArgsCard, RuntimeEnvCard, ServerSettingsCard } from "./OptionCards";
import { ModPlanCard, type DepWarning } from "./ModPlanCard";
import { PREVIEW_ROWS } from "./constants";

export function PlanReviewView({
    taskId,
    manifest,
}: {
    taskId: string;
    manifest: PackManifest;
}) {
    const t = useT();
    const [options, setOptions] = useState<ConversionOptions | null>(null);
    const [plan, setPlan] = useState<PlanMod[]>([]);
    const [packTree, setPackTree] = useState<PackDirTree>({ dirs: [], files: [] });
    /** 源包是否还解析得动：文件被移走时不能说「包内没有资源」，只能说不读得到 */
    const [packParsed, setPackParsed] = useState(false);
    const [tab, setTab] = useState<ModDisposition>("remove");
    /** 弹窗视角与开关分两个状态：关闭时若跟着清视角，退场那 200ms 会看到清单突然换一批 */
    const [listOpen, setListOpen] = useState(false);
    const [listFocus, setListFocus] = useState<ListFocus>("remove");

    useEffect(() => {
        let alive = true;
        void api.getTask(taskId).then((t) => alive && t && setOptions(t.options));
        void api.getTaskPlan(taskId).then((p) => alive && setPlan(p));
        // 目录树要重解析源包才有：先把后端指针对准这个包，源文件已不在时树留空
        void api
            .ensureParsed(manifest)
            .then((ok) => {
                if (!alive) return { dirs: [], files: [] };
                setPackParsed(ok);
                return ok ? api.listPackDirs() : { dirs: [], files: [] };
            })
            .then((tree) => alive && setPackTree(tree));
        return () => {
            alive = false;
        };
    }, [taskId, manifest]);

    const counts = useMemo(
        () => ({
            remove: plan.filter((m) => m.disposition === "remove").length,
            keep: plan.filter((m) => m.disposition === "keep").length,
            add: plan.filter((m) => m.disposition === "add").length,
            addTotal: plan.filter((m) => m.disposition === "add").length,
        }),
        [plan]
    );

    const previewRows = useMemo(
        () => reviewFirst(plan.filter((m) => m.disposition === tab)).slice(0, PREVIEW_ROWS),
        [plan, tab]
    );

    /** 「本地」徽章的依据在存档里就是 `localPath` 有没有值；不传则把本地 jar 行认成下载来的 */
    const localIds = useMemo(
        () => new Set(plan.filter((m) => m.localPath).map((m) => m.id)),
        [plan]
    );

    /** 反向依赖警告：保留/新增行依赖了被剔除的行（存档里不会有停用行，停用行从未参与构建） */
    const depWarnings = useMemo<DepWarning[]>(() => {
        const byId = new Map(plan.map((m) => [m.id, m]));
        const groups = new Map<string, DepWarning>();
        for (const m of plan) {
            if (m.disposition === "remove") continue;
            for (const d of m.depends ?? []) {
                const t = byId.get(d);
                if (t?.disposition === "remove" || t?.disabled) {
                    const g = groups.get(d) ?? { missing: t, hosts: [] };
                    g.hosts.push(m);
                    groups.set(d, g);
                }
            }
        }
        return [...groups.values()];
    }, [plan]);

    if (!options) {
        return (
            <Panel className="items-center py-16">
                <span className="h-4 w-40 animate-pulse rounded bg-stroke" />
            </Panel>
        );
    }

    // 只读视图不写任何东西：写入口在控件层已被摘掉，这两枚占位只为满足卡片共用的类型形状
    const patch = (_: Partial<ConversionOptions>) => {};
    const emptySelects: SelectOption[] = [];
    const loader = loaderLabel(manifest.loader);

    return (
        <>
            <RuntimeEnvCard
                options={options}
                patch={patch}
                manifest={manifest}
                loader={loader}
                mcOptions={emptySelects}
                loaderOptions={emptySelects}
                readOnly
            />
            <ModPlanCard
                tab={tab}
                onTab={setTab}
                counts={counts}
                rows={previewRows}
                classifying={false}
                totalRows={plan.length}
                readingLabel=""
                emptyLabel={t("convert.plan-saved", "该任务暂无可用的方案记录")}
                depWarnings={depWarnings}
                localIds={localIds}
                removableIds={new Set<string>()}
                manualEdits={0}
                confirmClear={false}
                readOnly
                onOpenList={(f) => {
                    setListFocus(f);
                    setListOpen(true);
                }}
            />
            <KeepDirsCard
                options={options}
                packTree={packTree}
                parsed={packParsed}
                readOnly
            />
            <LaunchArgsCard options={options} patch={patch} readOnly />
            <ServerSettingsCard options={options} patch={patch} readOnly />

            <PlanListModal
                open={listOpen}
                onClose={() => setListOpen(false)}
                focus={listFocus}
                readOnly
                mods={plan.filter((m) => m.disposition === listFocus)}
            />
        </>
    );
}
