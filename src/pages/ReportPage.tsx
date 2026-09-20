/**
 * 转换报告页 Report（SS.pen `Q45BPj`）
 *
 * 与 Convert 同构的两栏：左列（gap16）三卡 = 转换结果(gap14) / 变更明细(gap12) / 下一步(gap12)，
 * 右栏 280 = 输出与操作卡。全部数值来自 getReport + getTask，不写死设计稿文案。
 */
import { Archive, Check, FileText, Minus, Plus } from "lucide-react";
import { useEffect, useState } from "react";
import * as api from "@/lib/api";
import { useNavigation } from "@/lib/navigation";
import { formatClock, formatDuration, formatSize, loaderLabel, truncateMiddle } from "@/lib/format";
import type { ConversionReport, ConversionTask } from "@/lib/types";
import {
    Btn,
    ChangeRow,
    Divider,
    LinkBtn,
    MiniMeta,
    PageHeader,
    Panel,
    PanelHead,
    TagChip,
} from "@/components/design/ui";

/** HH:MM（"完成于 14:32"） */
const hm = (t: number) => formatClock(t).slice(0, 5);

export function ReportPage() {
    const { entry, navigate, switchPrimary } = useNavigation();
    const taskId = entry.params?.taskId as string | undefined;

    const [report, setReport] = useState<ConversionReport | null>(null);
    const [task, setTask] = useState<ConversionTask | null>(null);
    const [outputDir, setOutputDir] = useState<string | null>(null);
    const [copied, setCopied] = useState(false);

    useEffect(() => {
        if (!taskId) return;
        let alive = true;
        void Promise.all([api.getReport(taskId), api.getTask(taskId), api.getSettings()]).then(
            ([r, t, s]) => {
                if (!alive) return;
                setOutputDir(t?.options.outputOverride?.trim() || s.outputDir);
                if (t) setTask(t);
                if (r) setReport(r);
            }
        );
        return () => {
            alive = false;
        };
    }, [taskId]);

    if (!report) {
        return (
            <div className="flex flex-col gap-5">
                <PageHeader title="转换报告" />
                <Panel className="items-center py-16">
                    <p className="text-[13px] leading-[20px] text-text-2">
                        报告尚未生成——任务可能仍在进行或已过期。
                    </p>
                    <div className="mt-1 flex items-center gap-2">
                        {taskId && (
                            <Btn size="sm" onClick={() => navigate("task", { taskId })}>
                                查看任务详情
                            </Btn>
                        )}
                        <Btn variant="primary" size="sm" onClick={() => switchPrimary("tasks")}>
                            返回任务列表
                        </Btn>
                    </div>
                </Panel>
            </div>
        );
    }

    const o = report.options;
    const loader = task ? loaderLabel(task.pack.loader) : "Fabric";
    const duration = formatDuration(report.durationSec * 1000);
    const finishedAt = task?.finishedAt;

    /** 复制转换方案：把本次转换的可复现参数写进剪贴板 */
    const copyPlan = async () => {
        const text = [
            `整合包: ${task?.pack.fileName ?? "-"}`,
            `输出: ${report.outputFileName}（${formatSize(report.outputSizeBytes)}）`,
            `Minecraft: ${o.mcVersion}`,
            `加载器: ${loader} ${o.loaderVersion}`,
            `Java: ${o.javaVersion}`,
            `内存: ${Math.round(o.memoryMb / 1024)} GB`,
            `启动脚本: ${o.generateScripts ? "start.sh / start.bat" : "未生成"}`,
            `--nogui: ${o.nogui ? "开" : "关"} · EULA: ${o.agreeEula ? "自动接受" : "手动"}`,
            `变更: 剔除 ${report.removed} / 保留 ${report.kept} / 新增 ${report.added}`,
            report.pendingReview.length ? `待人工确认: ${report.pendingReview.join("、")}` : "",
        ]
            .filter(Boolean)
            .join("\n");
        await navigator.clipboard.writeText(text);
        setCopied(true);
        window.setTimeout(() => setCopied(false), 2000);
    };

    const openOutput = () => outputDir && void api.openDir(outputDir);

    return (
        <div className="flex flex-col gap-5">
            <PageHeader
                title="转换报告"
                sub={`${truncateMiddle(report.outputFileName, 36)} 已生成 · ${loader} · Minecraft ${o.mcVersion} · 总耗时 ${duration}`}
            />

            <div className="flex items-start gap-5">
                {/* 左列：转换结果 / 变更明细 / 下一步 */}
                <div className="flex min-w-0 flex-1 flex-col gap-4">
                    <Panel gap={14}>
                        <PanelHead title="转换结果" />

                        <div className="flex w-full items-center gap-2">
                            <Check className="size-3.5 shrink-0 text-emerald" />
                            <span className="shrink-0 text-[13px] leading-[20px] font-semibold text-emerald">
                                转换成功
                            </span>
                            <span className="min-w-0 truncate text-[12px] leading-[18px] font-normal text-text-2">
                                · 总耗时 {duration}
                                {finishedAt ? ` · 完成于 ${hm(finishedAt)}` : ""}
                            </span>
                        </div>

                        <div className="flex w-full items-center gap-2">
                            <FileText className="size-3.5 shrink-0 text-text-3" />
                            <span className="min-w-0 flex-1 truncate font-mono text-[12px] leading-[18px] font-medium text-text-1">
                                {truncateMiddle(report.outputFileName, 44)}
                            </span>
                            <TagChip className="py-0.5 font-normal">{formatSize(report.outputSizeBytes)}</TagChip>
                        </div>

                        <div className="flex w-full items-center gap-2">
                            <Archive className="size-3.5 shrink-0 text-text-3" />
                            <span className="min-w-0 flex-1 truncate font-mono text-[11px] leading-[16px] font-normal text-text-2">
                                {outputDir ?? "—"}
                            </span>
                            <LinkBtn size="sm" onClick={openOutput}>
                                打开文件夹
                            </LinkBtn>
                        </div>

                        <Divider />

                        <div className="flex w-full gap-6">
                            <MiniMeta label="加载器" value={`${loader} ${o.loaderVersion}`} />
                            <MiniMeta label="游戏版本" value={`Minecraft ${o.mcVersion}`} />
                            <MiniMeta label="Java" value={`Java ${o.javaVersion}`} />
                            <MiniMeta
                                label="启动脚本"
                                value={o.generateScripts ? "start.sh / start.bat" : "未生成"}
                            />
                        </div>
                    </Panel>

                    <Panel gap={12}>
                        <PanelHead title="变更明细" />
                        <ChangeRow
                            icon={Minus}
                            tone="gold"
                            title="剔除客户端专属模组"
                            sub="依据端证据（作者声明 / jar 自证 / 平台反查）"
                            count={report.removed}
                        />
                        <ChangeRow
                            icon={Check}
                            tone="emerald"
                            title="保留服务端兼容模组"
                            sub="含服务端必需的基础库"
                            count={report.kept}
                        />
                        <ChangeRow
                            icon={Plus}
                            tone="accent"
                            title="新增服务端依赖"
                            sub={loader === "Fabric" ? "Fabric Server Launcher 等" : "服务端启动器与前置库"}
                            count={report.added}
                        />

                        {report.pendingReview.length > 0 && (
                            <div className="flex w-full items-center gap-2.5">
                                <span className="size-2 shrink-0 rounded-full bg-gold" />
                                <span className="min-w-0 flex-1 truncate text-[12px] leading-[18px] font-normal text-text-2">
                                    待人工确认：{report.pendingReview.join("、")} 客户端/服务端两用，已默认保留
                                </span>
                                <TagChip className="py-0.5 text-gold">
                                    {report.pendingReview.length} 项
                                </TagChip>
                            </div>
                        )}

                        <LinkBtn
                            chevron
                            className="self-start"
                            onClick={() => taskId && navigate("task", { taskId })}
                        >
                            查看完整变更日志
                        </LinkBtn>
                    </Panel>

                    <Panel gap={12}>
                        <PanelHead title="下一步" />
                        <Step index={1} text={`将 ${report.outputFileName} 上传并部署到服务器目录`} />
                        <Step
                            index={2}
                            text={
                                o.generateScripts
                                    ? "运行随包生成的 start.sh / start.bat"
                                    : `手动以 java -Xmx${Math.round(o.memoryMb / 1024)}G 启动服务端核心`
                            }
                        />
                        <Step
                            index={3}
                            text={
                                o.agreeEula
                                    ? "首次启动后核对日志（eula=true 已自动写入）"
                                    : "首次启动前手动在 eula.txt 写入 eula=true"
                            }
                        />
                    </Panel>
                </div>

                {/* 右栏：输出与操作 */}
                <aside className="w-[280px] shrink-0">
                    <Panel gap={12}>
                        <PanelHead title="输出与操作" />

                        <div className="flex w-full items-center gap-2">
                            <FileText className="size-3.5 shrink-0 text-text-3" />
                            <span className="min-w-0 flex-1 truncate font-mono text-[11px] leading-[16px] font-medium text-text-1">
                                {truncateMiddle(report.outputFileName, 36)}
                            </span>
                        </div>
                        <div className="flex w-full items-center justify-between gap-3">
                            <span className="shrink-0 text-[11px] leading-[16px] font-normal text-text-3">
                                大小 / 耗时
                            </span>
                            <span className="truncate font-mono text-[11px] leading-[16px] font-medium text-text-2">
                                {formatSize(report.outputSizeBytes)} · {duration}
                            </span>
                        </div>

                        <Divider />

                        <Btn variant="primary" full className="font-semibold" onClick={openOutput}>
                            打开输出位置
                        </Btn>
                        <Btn size="sm" full className="bg-surface text-text-1" onClick={() => void copyPlan()}>
                            {copied ? "已复制方案" : "复制转换方案"}
                        </Btn>
                        <Btn size="sm" full onClick={() => switchPrimary("tasks")}>
                            返回任务列表
                        </Btn>
                    </Panel>
                </aside>
            </div>
        </div>
    );
}

/** 下一步条目：20×20 圆形序号（$surface-2 + 等宽 10/600）+ 12/500 正文 */
function Step({ index, text }: { index: number; text: string }) {
    return (
        <div className="flex w-full items-center gap-2.5">
            <span className="flex size-5 shrink-0 items-center justify-center rounded-full bg-surface-2 font-mono text-[10px] leading-[14px] font-semibold text-text-2">
                {index}
            </span>
            <span className="min-w-0 flex-1 text-[12px] leading-[18px] font-medium text-text-1">
                {text}
            </span>
        </div>
    );
}
