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
import * as api from "@/lib/api";
import { t, useBackendText, tSource, useT } from "@/lib/i18n";
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
    Collapse,
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

/** 游戏模式：`o.gamemode` 的原始枚举值不翻（它是查表用的线值，认不出的值要原样回落），只翻展示那一格。
 *  表建在函数里、每格一条 `t(字面量)`：顶层建表会把文案冻在首次加载的语言上。 */
function gamemodeLabel(gamemode: string): string {
    const label: Record<string, string> = {
        survival: t("convert.survival", "生存"),
        creative: t("convert.creative", "创造"),
        adventure: t("convert.adventure", "冒险"),
        spectator: t("convert.spectator", "旁观"),
    };
    return label[gamemode] ?? gamemode;
}

/** 难度档：同「游戏模式」的口径 */
function difficultyLabel(difficulty: string): string {
    const label: Record<string, string> = {
        peaceful: t("convert.peaceful", "和平"),
        easy: t("convert.easy", "简单"),
        normal: t("convert.normal", "普通"),
        hard: t("convert.hard", "困难"),
    };
    return label[difficulty] ?? difficulty;
}

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
    const t = useT();
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
                <PanelHead title={t("report.conversion-result", "转换结果")} />

                <div className="flex w-full items-center gap-2">
                    <Check className="size-3.5 shrink-0 text-emerald" />
                    <span className="shrink-0 text-[13px] leading-[20px] font-semibold text-emerald">
                        {t("report.conversion", "转换成功")}
                    </span>
                    <span className="min-w-0 truncate text-[12px] leading-[18px] font-normal text-text-2">
                        {t("report.duration-total", "· 总耗时 {{duration}}", { duration })}
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
                    <MiniMeta label={t("home.loader", "加载器")} value={`${loader} ${o.loaderVersion}`} />
                    {/* 本机装好这一档要在概况里看得见：它决定了「上传即跑」还是「首启联网自装」 */}
                    {report.installed && (
                        <MiniMeta label={t("report.loader-install", "加载器安装")} value={t("report.installed-locally", "本机已装好")} />
                    )}
                    <MiniMeta label={t("report.game-version", "游戏版本")} value={`Minecraft ${o.mcVersion}`} />
                    <MiniMeta label="Java" value={`Java ${o.javaVersion}`} />
                    <MiniMeta label={t("report.memory-limit", "内存上限")} value={gb(o.memoryMb)} />
                    {/* 完成时间必须带年月日：报告与任务存档是长期留存的，只有 HH:MM 无法定位是哪天 */}
                    {task.finishedAt && (
                        <MiniMeta label={t("report.finished", "完成于")} value={formatStamp(task.finishedAt)} />
                    )}
                    {/* 旧存档的报告没有文件数/包根清单（当时还没记），缺就整项不显示，别报 0 */}
                    {report.fileCount > 0 && (
                        <MiniMeta
                            label={t("report.packed-files", "打包文件")}
                            value={t("common.count", "{{count}} 个", { count: report.fileCount })}
                        />
                    )}
                    <MiniMeta
                        label={t("report.server-mods", "服务端模组")}
                        value={t("common.count", "{{count}} 个", { count: modCount })}
                    />
                </div>
            </Panel>

            <Panel gap={12}>
                <PanelHead
                    title={t("report.changes", "变更明细")}
                    right={
                        expandable ? (
                            <span className="text-[10px] leading-[14px] text-text-3">
                                {t("report.click-row", "点击行看清单")}
                            </span>
                        ) : undefined
                    }
                />
                <ChangeRow
                    icon={Minus}
                    tone="gold"
                    title={t("report.strip-client", "剔除客户端专属模组")}
                    sub={
                        report.removed > 0
                            ? t("report.side-evidence", "按端证据判定：jar 自证 / 平台反查 / 整合包声明")
                            : t("report.client-only", "本包没有需要剔除的客户端模组")
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
                    title={t("report.keep-server", "保留服务端兼容模组")}
                    sub={t("report.feature-mods", "两端通用的功能模组与前置库")}
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
                    title={t("report.add-server", "新增服务端依赖")}
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
                <PanelHead title={t("convert.server-settings", "服务端设置")} />
                <div className="grid w-full grid-cols-1 gap-x-6 gap-y-1.5 min-[520px]:grid-cols-2">
                    <InfoRow label={t("convert.server-port", "服务器端口")} value={String(o.serverPort)} />
                    <InfoRow label={t("convert.max-players", "最大人数")} value={String(o.maxPlayers)} />
                    <InfoRow label={t("convert.game-mode", "游戏模式")} value={gamemodeLabel(o.gamemode)} />
                    <InfoRow label={t("convert.difficulty", "难度")} value={difficultyLabel(o.difficulty)} />
                    <InfoRow
                        label={t("report.verify-accounts", "正版验证")}
                        value={o.onlineMode ? t("report.entry-2", "开启") : t("report.off-offline", "关闭（离线）")}
                    />
                    <InfoRow
                        label={t("report.gc-tuning", "GC 调优")}
                        value={o.useAikarFlags ? "Aikar's G1GC" : t("report.default", "默认")}
                    />
                    <InfoRow label={t("report.gui-mode", "界面模式")} value={o.nogui ? "--nogui" : t("report.gui", "带界面")} />
                    {o.levelSeed.trim() !== "" && (
                        <InfoRow label={t("report.world-seed", "世界种子")} value={o.levelSeed.trim()} />
                    )}
                    {o.motd.trim() !== "" && <InfoRow label="MOTD" value={o.motd.trim()} />}
                </div>
                {/* 这三行是卡内 flex 列（Panel gap=12）的整块挂卸 ⇒ 走 Collapse，gap 传 14 那张卡的 12。
                    上面 世界种子/MOTD 两格不在此列：它们在网格里，「补掉自己那一格」这件事在网格里不成立
                    （同排有没有兄弟决定那一格存不存在），硬演要么差 6px 要么多 6px */}
                <Collapse when={o.extraJvmArgs.trim() !== ""} gap={12}>
                    <InfoRow label={t("convert.extra-jvm", "附加 JVM 参数")} value={o.extraJvmArgs.trim()} />
                </Collapse>
                <Collapse when={o.keepDirs.length > 0 || o.keepFiles.length > 0} gap={12}>
                    <InfoRow
                        label={t("report.dirs-kept", "随包保留内容")}
                        // 目录带尾斜杠、根级文件就一个名字：一眼分得出勾的是哪一类
                        value={[...o.keepDirs.map((d) => `${d}/`), ...o.keepFiles].join("、")}
                    />
                </Collapse>
                {/* 包根文件来自 builder 实写清单：勾了脚本才会有 start.*，别按开关猜。
                    名字串会长，单独给一行可换行的展示位，不进右对齐的 InfoRow */}
                <Collapse when={report.generatedFiles.length > 0} gap={12}>
                    <div className="flex w-full flex-col gap-1">
                        <span className="text-[11px] leading-[16px] font-normal text-text-3">
                            {t("report.generated-root", "包根生成")}
                        </span>
                        <span className="break-words font-mono text-[11px] leading-[16px] font-medium text-text-1">
                            {report.generatedFiles.join("、")}
                        </span>
                    </div>
                </Collapse>
            </Panel>

            {/* 构建自检：逐项对账真实落盘产物。措辞口径——它证明的是「包齐不齐」，
                不是「开服能跑」，所以底部保留那句限定，别让绿勾被读成实机验证。
                整张卡的进出也走 Collapse：关掉自检时下面那张「下一步」不该当场蹿上来
                （本报告根容器是任务详情那个 flex 列，格距 gap-4=16） */}
            <Collapse when={report.checks.length > 0} gap={16}>
                <Panel gap={12}>
                    <PanelHead
                        title={t("report.build-self", "构建自检")}
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
                                    ? t("report.passed-count", "全部通过 · {{count}} 项", { count: report.checks.length })
                                    : t("report.count-item", "{{count}} 项需关注", {
                                          count: report.checks.filter((c) => c.status !== "pass")
                                              .length,
                                      })}
                            </span>
                        }
                    />
                    <div className="flex w-full flex-col gap-2">
                        {report.checks.map((c) => (
                            <CheckLine key={c.id} check={c} />
                        ))}
                    </div>
                    <span className="text-[11px] leading-[16px] text-text-3">
                        {t("report.checks-output", "离线核对产物完整性，未实机启动服务端")}
                    </span>
                </Panel>
            </Collapse>

            <Panel gap={12}>
                <PanelHead title={t("report.next-steps", "下一步")} />
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
    // 这段只写进剪贴板、不上界面（简报第 3 条：不进界面的串一律不包 t），所以档位词留中文原文，
    // 不借用上面那两个展示用的 gamemodeLabel / difficultyLabel —— 它们会跟着语言翻。
    const gamemodeCn: Record<string, string> = {
        survival: "生存",
        creative: "创造",
        adventure: "冒险",
        spectator: "旁观",
    };
    const difficultyCn: Record<string, string> = {
        peaceful: "和平",
        easy: "简单",
        normal: "普通",
        hard: "困难",
    };
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
        `服务端: 端口 ${o.serverPort} · 最多 ${o.maxPlayers} 人 · ${
            gamemodeCn[o.gamemode] ?? o.gamemode
        } · ${difficultyCn[o.difficulty] ?? o.difficulty} · 正版验证 ${
            o.onlineMode ? "开" : "关"
        }`,
        report.generatedFiles.length ? `包根文件: ${report.generatedFiles.join("、")}` : "",
        o.keepDirs.length || o.keepFiles.length
            ? `保留内容: ${[...o.keepDirs.map((d) => `${d}/`), ...o.keepFiles].join("、")}`
            : "",
        `变更: 剔除 ${report.removed} / 保留 ${report.kept} / 新增 ${report.added}`,
        report.pendingReview.length ? `待人工确认: ${report.pendingReview.join("、")}` : "",
    ]
        .filter(Boolean)
        .join("\n");
}

/**
 * 变更清单展开面板：展开与收起都走 Collapse（以前只有展开演了 height、收起是 `return null`
 * 当场卸载，等于半个折叠件）。
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
    const t = useT();
    /** 骨架期不铺真实行：rows 空是因为快照还没读到，不是「真的没有」，那时长度交给骨架占 */
    const shown = loading ? [] : rows.slice(0, LIST_CAP);
    return (
        <Collapse when={open && (loading || rows.length > 0)} gap={12}>
            <div className="log-scroll -mx-1 max-h-[220px] min-w-0 overflow-auto rounded-sm bg-surface-2/40 px-1 py-1">
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
                                    {t("report.sides-unknown", "未判定出两端 · 请核对服务端是否需要")}
                                </span>
                            )}
                        </span>
                        {m.autoSupplement && <TagChip tone="accent">{t("convert.auto-added", "自动补齐")}</TagChip>}
                        {!!m.sizeBytes && (
                            <span className="shrink-0 font-mono text-[11px] leading-[16px] text-text-3">
                                {formatSize(m.sizeBytes)}
                            </span>
                        )}
                    </ListRow>
                ))}
                {rows.length > shown.length && (
                    <p className="px-1 py-1.5 text-[10px] leading-[14px] text-text-3">
                        {t("report.count-listed", "另有 {{count}} 项未列出", { count: rows.length - shown.length })}
                    </p>
                )}
            </div>
        </Collapse>
    );
}

/** 「新增服务端依赖」的依据文案：有真实行就报真名，没行才说通用作用 */
function addedSub(loader: string, rows: PlanMod[], count: number): string {
    if (rows.length === 0)
        return count > 0 ? t("report.server-launcher", "服务端启动器与前置库") : t("report.deps-add", "本包无需补齐依赖");
    const auto = rows.filter((m) => m.autoSupplement).length;
    const head = rows
        .slice(0, 2)
        .map((m) => m.name)
        .join("、");
    // 整句 + 槽：拼出来的半截句子没法翻，两种排版各给一条键
    const names = rows.length > 2 ? t("report.names-etc", "{{names}} 等", { names: head }) : head;
    return auto > 0
        ? t("report.loader-server", "{{loader}} 服务端本体 + 自动补齐 {{count}} 项前置：{{names}}", {
              loader,
              count: auto,
              names,
          })
        : t("report.loader-server-itself", "{{loader}} 服务端本体：{{names}}", { loader, names });
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
    const steps: string[] = [
        t("report.upload-file", "将 {{file}} 上传到服务器，解压为独立目录后在其中启动", { file: fileName }),
    ];

    if (!o.agreeEula) {
        steps.push(
            t("report.before-starting", "启动前把包根 eula.txt 的 eula=false 改为 eula=true，否则服务端拒启")
        );
    }

    if (o.generateScripts) {
        steps.push(
            report.installed
                ? t(
                      "report.run-start", "解压后直接运行包内 start.bat / start.sh：加载器与依赖已在本机装好并打进包，目标机不需要再联网安装"
                  )
                : isForge
                  ? t(
                        "report.run-start-bat-start", "首次运行包内 start.bat / start.sh：会先自动执行 installer --installServer，之后同样用该脚本启动"
                    )
                  : t(
                        "report.run-start-bat", "首次运行包内 start.bat / start.sh：Fabric 那枚服务端 jar 会先联网装出加载器与前置库（需本机 Java 与网络），之后同样用该脚本启动"
                    )
        );
    } else if (report.installed && !report.startJar) {
        // 已装好又没生成脚本：起跳靠打进包的那两份参数文件（老 Forge 那态有 startJar，走下面那句 -jar）
        steps.push(
            t(
                "report.start-script-time", "本次未生成启动脚本：在解压目录执行 java <JVM 参数> @libraries/…/win_args.txt（Linux 用 unix_args.txt），参数文件随依赖树一起打进包"
            )
        );
    } else if (!report.installed && isForge) {
        // 未装又没脚本：包根那枚是安装器，`java -jar` 它不会开服 —— 先 --installServer 装出服务端
        const jar = report.startJar ?? "installer.jar";
        steps.push(
            t(
                "report.start-script", "本次未生成启动脚本：在解压目录执行 java -jar {{jar}} --installServer 联网装出服务端，再用它生成的 run.bat / run.sh 启动",
                { jar }
            )
        );
    } else {
        const jar = report.startJar ?? t("report.server-jar", "服务端 jar");
        steps.push(
            // 括号补充语直接接在后面（不留空格，中文原文逐字不变）：英文档里那条译文自带前导空格
            t("report.extracted-folder", "在解压目录执行 java -Xmx{{memory}}M -jar {{jar}}{{nogui}}{{fabricNote}}{{aikarNote}}", {
                memory: o.memoryMb,
                jar,
                nogui: o.nogui ? " nogui" : "",
                // Fabric 未装那态指的是官方服务端 jar：它首启会自装，别让人以为卡住了
                fabricNote:
                    !report.installed && !isForge
                        ? t("report.fabric-server", "（Fabric 服务端 jar 首次运行会联网装出加载器）")
                        : "",
                aikarNote: o.useAikarFlags
                    ? t("report.add-aikar", "（Aikar 的 G1GC 参数需自行补在 -Xmx 之后）")
                    : "",
            })
        );
    }

    steps.push(t("report.public-lan", "公网或局域网联机：在路由器/云安全组放行 TCP {{port}}", { port: o.serverPort }));

    steps.push(
        report.pendingReview.length > 0
            ? t(
                  "report.first-run", "首启核对日志：应加载 {{count}} 个模组、无红字报错，并确认 {{pending}} 项待判定模组服务端是否需要",
                  { count: modCount, pending: report.pendingReview.length }
              )
            : t("report.first-run-log", "首启核对日志：应加载 {{count}} 个模组且无红字报错", { count: modCount })
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
 *
 * `label` 是后端算好的固定中文：显示处 `tSource(raw)` 按「中文 → 键」表查目录（表里没有就原样出中文，
 * 行为不变），下面那串 i18n 就地标记登记的就是它们。`detail` 走 `detailMsg`（后端发的「模板 + 参数」，
 * 见 `lib/types/l10n.ts`）：带数字/文件名的句子只有这样才能翻，旧快照没这一项时照显示中文原句。
 * `items` 是文件名/目录名这类**对象名**，按原文显示（deps 与 keep 那两条拼了中文说明，
 * 要翻得把清单也结构化成模板 + 参数，代价与收益不成比例）。
 */
function CheckLine({ check }: { check: CheckResult }) {
    const bt = useBackendText();
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
                    {/*i18n:取件完整*/}
                    {/*i18n:jar 可用*/}
                    {/*i18n:依赖闭合*/}
                    {/*i18n:启动指向*/}
                    {/*i18n:Loader 就位*/}
                    {/*i18n:包根文件*/}
                    {/*i18n:report.kept-dirs=保留内容*/}
                    <span className="shrink-0 text-[11px] leading-[16px] font-semibold text-text-1">
                        {tSource(check.label)}
                    </span>
                    <span className="min-w-0 break-words text-[11px] leading-[16px] text-text-2">
                        {bt(check.detail, check.detailMsg?.key, check.detailMsg?.args)}
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
