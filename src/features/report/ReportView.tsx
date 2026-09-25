/**
 * 转换报告视图（SS.pen `Q45BPj` 的左列）：任务详情「结果」签的全部内容。
 *
 * 它不再是独立页面，也不再自带右栏：动作（打开输出位置 / 复制转换方案 / 返回任务列表）
 * 全部归任务详情右栏那个唯一动作区，这里只呈现事实。
 * 数据由任务详情一次装载后传入（report + task + 产物路径），这里不再拉第二遍；
 * 「下一步」按本次真实选项生成条目（序号随之重排），开关没踩到的坑不占步骤位。
 */
import { AlertTriangle, Archive, Check, CircleCheck, CircleX, FileText, Minus, Plus } from "lucide-react";
import { useEffect, useState } from "react";
import { motion } from "motion/react";
import * as api from "@/lib/api";
import { formatDuration, formatSize, formatStamp, loaderLabel, truncateMiddle } from "@/lib/format";
import type {
    CheckResult,
    ConversionReport,
    ConversionTask,
    ModDisposition,
    PlanMod,
} from "@/lib/types";
import { cn } from "@/lib/utils";
import {
    ChangeRow,
    Divider,
    InfoRow,
    ListRow,
    MiniMeta,
    Panel,
    PanelHead,
    TagChip,
    Tip,
    TIP_TRIGGER,
} from "@/components/ui";

/** 展开清单一次最多铺多少行：真实整合包动辄几百个模组，全铺会把报告撑成一堵墙 */
const LIST_CAP = 200;

const GAMEMODE_CN: Record<string, string> = {
    survival: "生存",
    creative: "创造",
    adventure: "冒险",
    spectator: "旁观",
};
const DIFFICULTY_CN: Record<string, string> = {
    peaceful: "和平",
    easy: "简单",
    normal: "普通",
    hard: "困难",
};

/** 4096MB → "4 GB"，6144MB → "6 GB"，1536MB → "1.5 GB" */
const gb = (mb: number) => `${+(mb / 1024).toFixed(1)} GB`;

export function ReportView({
    taskId,
    report,
    task,
    outPath,
}: {
    taskId: string;
    /** 由任务详情一次装载：右栏的「复制转换方案」用的是同一份数据，不该再拉一遍 */
    report: ConversionReport;
    task: ConversionTask;
    /** 产物完整路径：同样由上层算好（打开输出位置要用它定位目录） */
    outPath: string | null;
}) {
    /** 变更明细的展开态：一次只开一行，收拢即卸载 */
    const [openKey, setOpenKey] = useState<ModDisposition | null>(null);
    /**
     * 方案快照：只有这里的三行展开清单用得到，独立于上层装载。
     * null = 还没读到（≠ 读到了空清单：旧存档没有快照时后端就返回 []），
     * 首帧拿 `[]` 当「没有清单」会让三行的可点状态先关后开，看着像闪一下。
     */
    const [plan, setPlan] = useState<PlanMod[] | null>(null);

    useEffect(() => {
        let alive = true;
        void api
            .getTaskPlan(taskId)
            .then((p) => alive && setPlan(p))
            // 读失败按「确实没有快照」收口，不能让三行停在可点 + 骨架上不动
            .catch(() => alive && setPlan([]));
        return () => {
            alive = false;
        };
    }, [taskId]);

    const o = report.options;
    const loader = loaderLabel(task.pack.loader);
    const isForge = task.pack.loader === "forge" || task.pack.loader === "neoforge";
    const duration = formatDuration(report.durationSec * 1000);
    /** 进服务端包的模组数 = 保留 + 新增（剔除项不算，首启核对日志用的就是这个数） */
    const modCount = report.kept + report.added;

    const rowsOf = (d: ModDisposition) => (plan ?? []).filter((m) => m.disposition === d);
    const toggle = (d: ModDisposition) => setOpenKey((cur) => (cur === d ? null : d));
    /**
     * 三行能不能展开：快照还没读到也算能（点进去先给骨架），只有「确认没有快照」才收回。
     * 计数（剔除/保留/新增）上层已经装载好了，行能不能点不该等快照才定 ——
     * 先渲染成不可点的纯文本、落地后再变按钮，就是一眼看得见的跳变。
     */
    const expandable = plan === null || plan.length > 0;

    const steps = buildSteps({
        fileName: report.outputFileName,
        isForge,
        modCount,
        report,
        o,
    });

    return (
        <>
            <Panel gap={14}>
                <PanelHead title="转换结果" />

                <div className="flex w-full items-center gap-2">
                    <Check className="size-3.5 shrink-0 text-emerald" />
                    <span className="shrink-0 text-[13px] leading-[20px] font-semibold text-emerald">
                        转换成功
                    </span>
                    <span className="min-w-0 truncate text-[12px] leading-[18px] font-normal text-text-2">
                        · 总耗时 {duration}
                    </span>
                </div>

                <div className="flex w-full items-center gap-2">
                    <FileText className="size-3.5 shrink-0 text-text-3" />
                    <span className="min-w-0 flex-1 truncate font-mono text-[12px] leading-[18px] font-medium text-text-1">
                        {truncateMiddle(report.outputFileName, 44)}
                    </span>
                    <TagChip className="py-0.5 font-normal">
                        {formatSize(report.outputSizeBytes)}
                    </TagChip>
                </div>

                <div className="flex w-full items-center gap-2">
                    <Archive className="size-3.5 shrink-0 text-text-3" />
                    {/* 路径截断必须掐中间：掐尾巴会把文件名截没，而文件名才是这行的答案。
                        完整路径走 Tip（内层才是截断件：外层裁刀开着，气泡塞进去会被裁掉） */}
                    <span className={cn(TIP_TRIGGER, "flex min-w-0 flex-1 items-center")}>
                        <span className="min-w-0 truncate font-mono text-[11px] leading-[16px] font-normal text-text-2">
                            {outPath ? truncateMiddle(outPath, 58) : "—"}
                        </span>
                        <Tip label={outPath ?? undefined} align="start" wide />
                    </span>
                </div>

                <Divider />

                {/* 窄窗口下六个格子换行，不靠横向挤压保排版 */}
                <div className="flex w-full flex-wrap gap-x-6 gap-y-3">
                    <MiniMeta label="加载器" value={`${loader} ${o.loaderVersion}`} />
                    {/* 本机装好这一档要在概况里看得见：它决定了「上传即跑」还是「首启联网自装」 */}
                    {report.installed && <MiniMeta label="加载器安装" value="本机已装好" />}
                    <MiniMeta label="游戏版本" value={`Minecraft ${o.mcVersion}`} />
                    <MiniMeta label="Java" value={`Java ${o.javaVersion}`} />
                    <MiniMeta label="内存上限" value={gb(o.memoryMb)} />
                    {/* 完成时间必须带年月日：报告与任务存档是长期留存的，只有 HH:MM 无法定位是哪天 */}
                    {task.finishedAt && (
                        <MiniMeta label="完成于" value={formatStamp(task.finishedAt)} />
                    )}
                    {/* 旧存档的报告没有文件数/包根清单（当时还没记），缺就整项不显示，别报 0 */}
                    {report.fileCount > 0 && (
                        <MiniMeta label="打包文件" value={`${report.fileCount} 个`} />
                    )}
                    <MiniMeta label="服务端模组" value={`${modCount} 个`} />
                </div>
            </Panel>

            <Panel gap={12}>
                <PanelHead
                    title="变更明细"
                    right={
                        expandable ? (
                            <span className="text-[10px] leading-[14px] text-text-3">
                                点击行看清单
                            </span>
                        ) : undefined
                    }
                />
                <ChangeRow
                    icon={Minus}
                    tone="gold"
                    title="剔除客户端专属模组"
                    sub={
                        report.removed > 0
                            ? "按端证据判定：jar 自证 / 平台反查 / 整合包声明"
                            : "本包没有需要剔除的客户端模组"
                    }
                    count={report.removed}
                    open={openKey === "remove"}
                    onClick={expandable ? () => toggle("remove") : undefined}
                />
                <ChangeList
                    rows={rowsOf("remove")}
                    open={openKey === "remove"}
                    loading={plan === null}
                />
                <ChangeRow
                    icon={Check}
                    tone="emerald"
                    title="保留服务端兼容模组"
                    sub="两端通用的功能模组与前置库"
                    count={report.kept}
                    open={openKey === "keep"}
                    onClick={expandable ? () => toggle("keep") : undefined}
                />
                <ChangeList
                    rows={rowsOf("keep")}
                    open={openKey === "keep"}
                    loading={plan === null}
                />
                <ChangeRow
                    icon={Plus}
                    tone="accent"
                    title="新增服务端依赖"
                    sub={addedSub(loader, rowsOf("add"), report.added)}
                    count={report.added}
                    open={openKey === "add"}
                    onClick={expandable ? () => toggle("add") : undefined}
                />
                <ChangeList
                    rows={rowsOf("add")}
                    open={openKey === "add"}
                    loading={plan === null}
                />

                {/* 待人工确认项不再单列成一行名单（几十项会顶爆这一行）：
                    转换时它们已被归进剔除并置顶，要看就在上面「剔除」行展开，行内带金色说明；
                    总数由「下一步」的最后一条报。 */}
            </Panel>

            <Panel gap={12}>
                <PanelHead title="服务端设置" />
                <div className="grid w-full grid-cols-1 gap-x-6 gap-y-1.5 min-[520px]:grid-cols-2">
                    <InfoRow label="服务器端口" value={String(o.serverPort)} />
                    <InfoRow label="最大人数" value={String(o.maxPlayers)} />
                    <InfoRow label="游戏模式" value={GAMEMODE_CN[o.gamemode] ?? o.gamemode} />
                    <InfoRow label="难度" value={DIFFICULTY_CN[o.difficulty] ?? o.difficulty} />
                    <InfoRow label="正版验证" value={o.onlineMode ? "开启" : "关闭（离线）"} />
                    <InfoRow label="GC 调优" value={o.useAikarFlags ? "Aikar's G1GC" : "默认"} />
                    <InfoRow label="界面模式" value={o.nogui ? "--nogui" : "带界面"} />
                    {o.levelSeed.trim() !== "" && (
                        <InfoRow label="世界种子" value={o.levelSeed.trim()} />
                    )}
                    {o.motd.trim() !== "" && <InfoRow label="MOTD" value={o.motd.trim()} />}
                </div>
                {o.extraJvmArgs.trim() !== "" && (
                    <InfoRow label="附加 JVM 参数" value={o.extraJvmArgs.trim()} />
                )}
                {o.keepDirs.length > 0 && (
                    <InfoRow label="随包保留目录" value={o.keepDirs.join("、")} />
                )}
                {/* 包根文件来自 builder 实写清单：勾了脚本才会有 start.*，别按开关猜。
                    名字串会长，单独给一行可换行的展示位，不进右对齐的 InfoRow */}
                {report.generatedFiles.length > 0 && (
                    <div className="flex w-full flex-col gap-1">
                        <span className="text-[11px] leading-[16px] font-normal text-text-3">
                            包根生成
                        </span>
                        <span className="break-words font-mono text-[11px] leading-[16px] font-medium text-text-1">
                            {report.generatedFiles.join("、")}
                        </span>
                    </div>
                )}
            </Panel>

            {/* 构建自检：逐项对账真实落盘产物。措辞口径——它证明的是「包齐不齐」，
                不是「开服能跑」，所以底部保留那句限定，别让绿勾被读成实机验证 */}
            {report.checks.length > 0 && (
                <Panel gap={12}>
                    <PanelHead
                        title="构建自检"
                        right={
                            <span
                                className={`text-[11px] leading-[16px] font-medium ${
                                    report.checks.every((c) => c.status === "pass")
                                        ? "text-emerald"
                                        : report.checks.some((c) => c.status === "fail")
                                          ? "text-redstone"
                                          : "text-gold"
                                }`}
                            >
                                {report.checks.every((c) => c.status === "pass")
                                    ? `全部通过 · ${report.checks.length} 项`
                                    : `${report.checks.filter((c) => c.status !== "pass").length} 项需关注`}
                            </span>
                        }
                    />
                    <div className="flex w-full flex-col gap-2">
                        {report.checks.map((c) => (
                            <CheckLine key={c.id} check={c} />
                        ))}
                    </div>
                    <span className="text-[11px] leading-[16px] text-text-3">
                        离线核对产物完整性，未实机启动服务端
                    </span>
                </Panel>
            )}

            <Panel gap={12}>
                <PanelHead title="下一步" />
                {steps.map((text, i) => (
                    <Step key={text} index={i + 1} text={text} />
                ))}
            </Panel>
        </>
    );
}

/**
 * 复制转换方案的正文：一次转换的可复现参数（供任务详情右栏写进剪贴板）。
 * 口径与报告卡一致，全部取自 report/task 实数，不在这段里补判断。
 */
export function buildPlanSummary(
    task: ConversionTask,
    report: ConversionReport,
    outPath: string | null
): string {
    const o = report.options;
    return [
        `整合包: ${task.pack.fileName}`,
        `输出: ${outPath ?? report.outputFileName}（${formatSize(report.outputSizeBytes)}${
            report.fileCount ? ` · ${report.fileCount} 个文件` : ""
        }）`,
        `Minecraft: ${o.mcVersion}`,
        `加载器: ${loaderLabel(task.pack.loader)} ${o.loaderVersion}`,
        `Java: ${o.javaVersion}`,
        `内存: ${gb(o.memoryMb)}`,
        `启动脚本: ${o.generateScripts ? "start.sh / start.bat" : "未生成"}`,
        `JVM: -Xmx${o.memoryMb}M${o.useAikarFlags ? " + Aikar's G1GC" : ""}${
            o.extraJvmArgs ? ` ${o.extraJvmArgs}` : ""
        }`,
        `--nogui: ${o.nogui ? "开" : "关"} · EULA: ${o.agreeEula ? "自动接受" : "手动"}`,
        `服务端: 端口 ${o.serverPort} · 最多 ${o.maxPlayers} 人 · ${GAMEMODE_CN[o.gamemode] ?? o.gamemode} · ${
            DIFFICULTY_CN[o.difficulty] ?? o.difficulty
        } · 正版验证 ${o.onlineMode ? "开" : "关"}`,
        report.generatedFiles.length ? `包根文件: ${report.generatedFiles.join("、")}` : "",
        o.keepDirs.length ? `保留目录: ${o.keepDirs.join("、")}` : "",
        `变更: 剔除 ${report.removed} / 保留 ${report.kept} / 新增 ${report.added}`,
        report.pendingReview.length ? `待人工确认: ${report.pendingReview.join("、")}` : "",
    ]
        .filter(Boolean)
        .join("\n");
}

/**
 * 变更清单展开面板：只在展开时挂载，收起即卸载，不留隐藏 DOM。
 * 快照还没读到时给三行骨架（点了没反应 = 用户以为这行不能点；先撑住高度，
 * 真实清单落地时行位不动，不会看着像跳）。
 */
function ChangeList({
    rows,
    open,
    loading,
}: {
    rows: PlanMod[];
    open: boolean;
    loading: boolean;
}) {
    if (!open) return null;
    if (!loading && rows.length === 0) return null;
    /** 骨架期不铺真实行：rows 空是因为快照还没读到，不是「真的没有」，那时长度交给骨架占 */
    const shown = loading ? [] : rows.slice(0, LIST_CAP);
    return (
        <motion.div
            initial={{ opacity: 0, height: 0 }}
            animate={{ opacity: 1, height: "auto" }}
            className="log-scroll -mx-1 max-h-[220px] min-w-0 overflow-auto rounded-md bg-surface-2/40 px-1 py-1"
        >
            {loading &&
                [0, 1, 2].map((i) => (
                    <div key={i} className="flex h-[46px] items-center gap-3 px-1">
                        <span className="h-[13px] flex-1 animate-pulse rounded bg-stroke" />
                        <span className="h-[11px] w-14 shrink-0 animate-pulse rounded bg-stroke-soft" />
                    </div>
                ))}
            {shown.map((m) => (
                <ListRow key={m.id} className="py-1.5">
                    <span className="flex min-w-0 flex-1 flex-col">
                        <span className="truncate font-mono text-[12px] leading-[18px] font-medium text-text-1">
                            {m.name} {m.version}
                        </span>
                        {m.needsReview && (
                            <span className="text-[10px] leading-[14px] text-gold">
                                未判定出两端 · 请核对服务端是否需要
                            </span>
                        )}
                    </span>
                    {m.autoSupplement && <TagChip tone="accent">自动补齐</TagChip>}
                    {!!m.sizeBytes && (
                        <span className="shrink-0 font-mono text-[11px] leading-[16px] text-text-3">
                            {formatSize(m.sizeBytes)}
                        </span>
                    )}
                </ListRow>
            ))}
            {rows.length > shown.length && (
                <p className="px-1 py-1.5 text-[10px] leading-[14px] text-text-3">
                    另有 {rows.length - shown.length} 项未列出
                </p>
            )}
        </motion.div>
    );
}

/** 「新增服务端依赖」的依据文案：有真实行就报真名，没行才说通用作用 */
function addedSub(loader: string, rows: PlanMod[], count: number): string {
    if (rows.length === 0) return count > 0 ? "服务端启动器与前置库" : "本包无需补齐依赖";
    const auto = rows.filter((m) => m.autoSupplement).length;
    const head = rows
        .slice(0, 2)
        .map((m) => m.name)
        .join("、");
    return `${loader} 服务端本体${auto > 0 ? ` + 自动补齐 ${auto} 项前置` : ""}：${head}${rows.length > 2 ? " 等" : ""}`;
}

/**
 * 「下一步」按本次真实选项生成，序号随条目数重排：
 * eula 已自动写成 true 就不再占一步，端口放行与首启核对才是每次都跑得上的动作。
 */
function buildSteps(a: {
    fileName: string;
    isForge: boolean;
    modCount: number;
    report: ConversionReport;
    o: ConversionReport["options"];
}): string[] {
    const { fileName, isForge, modCount, report, o } = a;
    const steps: string[] = [`将 ${fileName} 上传到服务器，解压为独立目录后在其中启动`];

    if (!o.agreeEula) {
        steps.push("启动前把包根 eula.txt 的 eula=false 改为 eula=true，否则服务端拒启");
    }

    if (o.generateScripts) {
        steps.push(
            report.installed
                ? "解压后直接运行包内 start.bat / start.sh：加载器与依赖已在本机装好并打进包，目标机不需要再联网安装"
                : isForge
                  ? "首次运行包内 start.bat / start.sh：会先自动执行 installer --installServer，之后同样用该脚本启动"
                  : "运行包内 start.bat（Windows）或 start.sh（Linux/macOS），JVM 参数与 --nogui 已按本次配置写好"
        );
    } else if (report.installed && !report.startJar) {
        // 已装好又没生成脚本：起跳靠打进包的那两份参数文件（老 Forge 那态有 startJar，走下面那句 -jar）
        steps.push(
            "本次未生成启动脚本：在解压目录执行 java <JVM 参数> @libraries/…/win_args.txt（Linux 用 unix_args.txt），参数文件随依赖树一起打进包"
        );
    } else {
        const jar = report.startJar ?? "服务端 jar";
        steps.push(
            `在解压目录执行 java -Xmx${o.memoryMb}M -jar ${jar}${o.nogui ? " nogui" : ""}${o.useAikarFlags ? "（Aikar 的 G1GC 参数需自行补在 -Xmx 之后）" : ""}`
        );
    }

    steps.push(`公网或局域网联机：在路由器/云安全组放行 TCP ${o.serverPort}`);

    steps.push(
        report.pendingReview.length > 0
            ? `首启核对日志：应加载 ${modCount} 个模组、无红字报错，并确认 ${report.pendingReview.length} 项待判定模组服务端是否需要`
            : `首启核对日志：应加载 ${modCount} 个模组且无红字报错`
    );
    return steps;
}

/** 下一步条目：20×20 圆形序号（$surface-2 + 等宽 10/600）+ 12/500 正文 */
function Step({ index, text }: { index: number; text: string }) {
    return (
        <div className="flex w-full items-start gap-2.5">
            <span className="flex size-5 shrink-0 items-center justify-center rounded-full bg-surface-2 font-mono text-[10px] leading-[14px] font-semibold text-text-2">
                {index}
            </span>
            <span className="min-w-0 flex-1 break-words text-[12px] leading-[18px] font-medium text-text-1">
                {text}
            </span>
        </div>
    );
}

/**
 * 自检单行：状态图标 + 项目名 + 带数字的结论；有对象清单时补一行等宽小字。
 * 明细全来自后端实数（缺哪几个文件、哪几个 jar 坏了），这里不补任何判断。
 */
function CheckLine({ check }: { check: CheckResult }) {
    const Icon =
        check.status === "pass" ? CircleCheck : check.status === "fail" ? CircleX : AlertTriangle;
    const tone =
        check.status === "pass"
            ? "text-emerald"
            : check.status === "fail"
              ? "text-redstone"
              : "text-gold";
    return (
        <div className="flex w-full items-start gap-2">
            <Icon className={`mt-[2px] size-3.5 shrink-0 ${tone}`} />
            <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                <div className="flex min-w-0 flex-wrap items-baseline gap-x-2">
                    <span className="shrink-0 text-[11px] leading-[16px] font-semibold text-text-1">
                        {check.label}
                    </span>
                    <span className="min-w-0 break-words text-[11px] leading-[16px] text-text-2">
                        {check.detail}
                    </span>
                </div>
                {(check.items?.length ?? 0) > 0 && (
                    <span className="break-words font-mono text-[10px] leading-[15px] text-text-3">
                        {check.items!.join("、")}
                    </span>
                )}
            </div>
        </div>
    );
}
