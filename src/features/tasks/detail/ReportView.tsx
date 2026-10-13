/**
 * 转换报告视图：任务详情「结果」签的全部内容（不是独立页，动作全部归详情右栏的唯一动作区）。
 * 数据由任务详情一次装载后传入，这里不再拉第二遍；「下一步」按本次真实选项生成条目。
 */
import { AlertTriangle, Archive, Check, CircleCheck, CircleX, FileText, Minus, Plus } from "lucide-react";
import { useEffect, useState } from "react";
import * as api from "@/lib/api";
import { activeLocale, backendText, t, useBackendText, tSource, useT } from "@/lib/i18n";
import { formatDuration, formatSize, formatStamp, loaderLabel, reviewHoldReason, truncateMiddle } from "@/lib/format";
import type {
    CheckResult,
    ConversionReport,
    ConversionTask,
    ModDisposition,
    PlanMod,
} from "@/lib/types";
import { holdsRootFile, ROOT_EULA } from "@/lib/types";
import { stageLabel } from "@/lib/rail-view";
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
    /** 本次探明拿不到字节、按「允许跳过」放过去的那几个名字（老快照同样没这一键 ⇒ 空） */
    const skipped = report.skippedMods ?? [];
    /** 进服务端包的模组数 = 保留 + 新增（剔除项不算，首启核对日志用的就是这个数）。
     *  再扣掉缺件跳过的那几个：上面两个计数按方案处置算，跳过的行处置还在 keep/add 里，
     *  不扣就是在教服主去日志里数一个包里根本没有的数 */
    const modCount = report.kept + report.added - skipped.length;
    /** 勾了包内同名根文件时让位的那几枚（老快照没这一键 ⇒ 空）：报告要说「沿用」而不是「生成」 */
    const reused = report.reusedRootFiles ?? [];

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

                {/* 缺件名单：上面三行报的是「方案怎么处置」，这一行报的是「实际少给了什么」——
                    CurseForge 那些不通过接口发放下载链的项目，勾选允许跳过后就是走这条路出包。
                    名字单独一行可换行，跟「包根沿用」同一形态，不进右对齐的计数行 */}
                <Collapse when={skipped.length > 0} gap={12}>
                    <div className="flex w-full flex-col gap-1">
                        <span className="text-[11px] leading-[16px] font-normal text-text-3">
                            {t("report.skipped-mods", "拿不到文件 · 本次未打包")}
                        </span>
                        <span className="break-words font-mono text-[11px] leading-[16px] font-medium text-redstone">
                            {skipped.join("、")}
                        </span>
                    </div>
                </Collapse>

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
                    名字串会长，单独给一行可换行的展示位，不进右对齐的 InfoRow。
                    让位那一半和它同位：上面那 8 项当时没进产物，读报告的人要能看到这条分界线 */}
                <Collapse when={report.generatedFiles.length > 0 || reused.length > 0} gap={12}>
                    <div className="flex w-full flex-col gap-1">
                        {report.generatedFiles.length > 0 && (
                            <>
                                <span className="text-[11px] leading-[16px] font-normal text-text-3">
                                    {t("report.generated-root", "包根生成")}
                                </span>
                                <span className="break-words font-mono text-[11px] leading-[16px] font-medium text-text-1">
                                    {report.generatedFiles.join("、")}
                                </span>
                            </>
                        )}
                        {reused.length > 0 && (
                            <>
                                <span className="text-[11px] leading-[16px] font-normal text-text-3">
                                    {t("report.reused-root", "包根沿用 · 本次未按配置生成")}
                                </span>
                                <span className="break-words font-mono text-[11px] leading-[16px] font-medium text-gold">
                                    {reused.join("、")}
                                </span>
                            </>
                        )}
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

/** 本档的排版件：一次成文取一趟，保证同一份文本里符号成套 */
function clipPunct() {
    const zh = activeLocale() !== "en-US";
    return {
        sep: zh ? "、" : ", ",
        head: zh ? "【" : "[",
        tail: zh ? "】" : "]",
        open: zh ? "（" : " (",
        close: zh ? "）" : ")",
    };
}

/** 到秒：贴出去要跟日志时间轴对得齐，`formatStamp` 只到分。数字与 `-/:` 三档同形，不过 `t` */
function stampSec(ms?: number | null): string {
    if (!ms) return "";
    const d = new Date(ms);
    const p = (n: number) => String(n).padStart(2, "0");
    return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
}

/** 空值整行不留：一份贴出去的清单里出现「MOTD: 」比少一行更碍事 */
function row(label: string, value: string | number | null | undefined): string {
    return value === null || value === undefined || value === "" ? "" : `${label}: ${value}`;
}

/**
 * 模板的视野：成文一次能看到的全部数据。
 * 派生件（排版件、计数、自检那两组）在这里算一趟，模板里的值函数只管取，不再各自判空。
 */
type ClipView = {
    task: ConversionTask;
    report: ConversionReport | null;
    outPath: string | null;
    /** 方案快照（`get_task_plan`）：报告只给计数，逐条列名要问它 */
    plan: PlanMod[] | null;
    o: ConversionTask["options"];
    p: ReturnType<typeof clipPunct>;
    /** 计数优先用报告实数；失败与取消没有报告，退回任务快照上那组（检测阶段之后就写） */
    counts: ConversionTask["counts"];
    checks: CheckResult[];
    notPassed: CheckResult[];
};

/** 模板的一条行：字段行 `标签: 值`（值为空 ⇒ 整行消失），或原样行（自检子行那种自带缩进） */
type ClipRow =
    | { label: string; value: (v: ClipView) => string | number | null | undefined }
    | { line: (v: ClipView) => string };

/** 模板的一节：节名 + 出场条件 + 行。节名必须是字面量 `t()`——扫描器只认字面量，键名进变量就采不到 */
type ClipSection = {
    title: string;
    /** 不写＝一定出；行全空时渲染器仍会连标题一起省掉 */
    when?: (v: ClipView) => boolean;
    rows: ClipRow[];
};

/** 是/否与状态词、自检标签都写成函数：模块级常量表会把词冻在首次加载的语言上 */
const yesNo = (v: boolean) => (v ? t("report.clip-yes", "是") : t("report.clip-no", "否"));

function clipStatusWord(status: ConversionTask["status"]): string {
    const words: Record<ConversionTask["status"], string> = {
        queued: t("lib.queued", "排队中"),
        running: t("common.converting", "转换中"),
        success: t("report.conversion", "转换成功"),
        failed: t("report.clip-failed", "转换失败"),
        cancelled: t("lib.canceled", "已取消"),
    };
    return words[status];
}

function clipCheckWord(status: CheckResult["status"]): string {
    const words: Record<CheckResult["status"], string> = {
        pass: t("report.clip-check-pass", "通过"),
        warn: t("report.clip-check-warn", "警告"),
        fail: t("report.clip-check-fail", "失败"),
    };
    return words[status];
}

/** 名字类数组：超过 40 个截住并报总数，免得一份参数表撑出几百行 */
function names(list: string[] | undefined): string {
    if (!list?.length) return "";
    const p = clipPunct();
    const cap = 40;
    if (list.length <= cap) return list.join(p.sep);
    return `${list.slice(0, cap).join(p.sep)}…${p.open}${t("report.clip-total-count", "共 {{count}} 项", { count: list.length })}${p.close}`;
}

/**
 * 新增模组逐条列名与版本：处置为 add 的方案行就是那批名字（只给值，标签由模板那一行给）。
 * 计数说有、快照却没列出来（旧存档没有方案）⇒ 明说「未能列出」，不静默给一行空清单。
 */
function addedMods(plan: PlanMod[] | null, addCount: number | undefined): string {
    if (!addCount) return "";
    const rows = (plan ?? []).filter((m) => m.disposition === "add");
    if (!rows.length) return t("report.clip-added-no-plan", "{{count}} 个（方案快照缺失，未能列出）", { count: addCount });
    return names(rows.map((m) => [m.name, m.version].filter(Boolean).join(" ")));
}

/** 渲染一节：条件不成立或行全空 ⇒ 空串，连标题一起消失，不留空壳 */
function renderClipSection(s: ClipSection, v: ClipView): string {
    if (s.when && !s.when(v)) return "";
    const body = s.rows
        .map((r) => ("label" in r ? row(r.label, r.value(v)) : r.line(v)))
        .filter(Boolean)
        .join("\n");
    if (!body) return "";
    return `${v.p.head}${s.title}${v.p.tail}\n${body}`;
}

/**
 * 「复制转换方案」的模板：八节，数组顺序即出文顺序。
 * 加一节＝加一条，调顺序＝挪一条，`when` 说清这一节什么时候出场，值一律 `(v) =>` 现取。
 * 节名与标签都写成字面量 `t()`：扫描器只认字面量，键一旦进变量就采不到、语言目录当场漏。
 */
function planClipTemplate(): ClipSection[] {
    return [
        {
            title: t("report.clip-sec-task", "任务"),
            rows: [
                { label: t("report.clip-task-id", "任务号"), value: (v) => v.task.id },
                { label: t("report.clip-status", "状态"), value: (v) => clipStatusWord(v.task.status) },
                { label: t("report.clip-pack", "源包"), value: (v) => v.task.pack.fileName },
                { label: t("report.clip-pack-path", "源包路径"), value: (v) => v.task.pack.sourcePath },
                {
                    label: t("report.clip-pack-size", "包体积"),
                    value: (v) =>
                        `${formatSize(v.task.pack.sizeBytes)} · ${t("report.clip-mod-count", "模组 {{count}} 个", { count: v.task.pack.modCount })}`,
                },
                { label: t("report.clip-created", "创建"), value: (v) => stampSec(v.task.createdAt) },
                { label: t("report.clip-started", "开始"), value: (v) => stampSec(v.task.startedAt) },
                { label: t("report.clip-finished", "结束"), value: (v) => stampSec(v.task.finishedAt) },
                {
                    label: t("report.clip-duration", "耗时"),
                    value: (v) =>
                        v.task.finishedAt
                            ? formatDuration(v.task.finishedAt - (v.task.startedAt ?? v.task.createdAt))
                            : "",
                },
            ],
        },
        {
            title: t("report.clip-sec-runtime", "运行环境"),
            rows: [
                { label: "Minecraft", value: (v) => v.o.mcVersion },
                { label: t("home.loader", "加载器"), value: (v) => `${loaderLabel(v.task.pack.loader)} ${v.o.loaderVersion}` },
                { label: t("report.clip-java-required", "Java 需求"), value: (v) => v.o.javaVersion },
                {
                    label: t("report.clip-java-used", "本次 Java"),
                    value: (v) =>
                        v.o.javaPath ||
                        `${t("convert.auto-select", "自动选择")}${v.p.open}${t("report.clip-java-auto-note", "需求线之上最低的一枚")}${v.p.close}`,
                },
                {
                    label: t("report.clip-install-loader", "本机安装加载器"),
                    value: (v) => yesNo(v.o.installLoaderLocally),
                },
                { label: t("report.memory-limit", "内存上限"), value: (v) => gb(v.o.memoryMb) },
                {
                    label: t("report.clip-jvm-args", "JVM 参数"),
                    value: (v) => `-Xmx${v.o.memoryMb}M${v.o.useAikarFlags ? " + Aikar's G1GC" : ""}`,
                },
                { label: t("convert.extra-jvm", "附加 JVM 参数"), value: (v) => v.o.extraJvmArgs },
                {
                    label: t("report.clip-scripts", "启动脚本"),
                    value: (v) =>
                        v.o.generateScripts ? "start.sh / start.bat" : t("report.clip-not-generated", "未生成"),
                },
                { label: t("report.clip-nogui", "无界面模式 --nogui"), value: (v) => yesNo(v.o.nogui) },
                {
                    label: "EULA",
                    value: (v) =>
                        v.o.agreeEula
                            ? t("report.clip-eula-auto", "自动写入 eula=true")
                            : t("report.clip-eula-manual", "手动接受"),
                },
                { label: t("report.clip-allow-missing", "允许跳过缺件"), value: (v) => yesNo(!!v.o.allowMissingMods) },
            ],
        },
        {
            title: t("convert.server-settings", "服务端设置"),
            rows: [
                { label: t("convert.server-port", "服务器端口"), value: (v) => v.o.serverPort },
                { label: t("convert.max-players", "最大人数"), value: (v) => v.o.maxPlayers },
                { label: "MOTD", value: (v) => v.o.motd },
                { label: t("convert.game-mode", "游戏模式"), value: (v) => gamemodeLabel(v.o.gamemode) },
                { label: t("convert.difficulty", "难度"), value: (v) => difficultyLabel(v.o.difficulty) },
                {
                    label: t("report.verify-accounts", "正版验证"),
                    value: (v) => (v.o.onlineMode ? t("report.entry-2", "开启") : t("report.off-offline", "关闭（离线）")),
                },
                {
                    label: t("report.world-seed", "世界种子"),
                    value: (v) => v.o.levelSeed.trim() || t("report.clip-seed-random", "随机"),
                },
            ],
        },
        {
            title: t("report.clip-sec-keep", "保留内容"),
            rows: [
                // 这一节恒出现：勾没勾是要复现的一等信息，「空」也得被明写成「无」
                {
                    label: t("report.clip-keep-dirs", "目录"),
                    value: (v) =>
                        v.o.keepDirs.length ? names(v.o.keepDirs.map((d) => `${d}/`)) : t("report.clip-none", "无"),
                },
                {
                    label: t("report.clip-keep-files", "文件"),
                    value: (v) => names(v.o.keepFiles) || t("report.clip-none", "无"),
                },
            ],
        },
        {
            title: t("report.clip-sec-plan", "转换方案"),
            rows: [
                {
                    label: t("report.clip-disposition", "模组处置"),
                    value: (v) =>
                        v.counts
                            ? t("report.clip-disposition-value", "剔除 {{remove}} · 保留 {{keep}} · 新增 {{add}}", {
                                  remove: v.counts.remove,
                                  keep: v.counts.keep,
                                  add: v.counts.add,
                              })
                            : "",
                },
                { label: t("report.clip-added-mods", "新增模组"), value: (v) => addedMods(v.plan, v.counts?.add) },
                { label: t("report.clip-pending-review", "待人工确认"), value: (v) => names(v.report?.pendingReview) },
            ],
        },
        {
            title: t("report.clip-sec-output", "产物"),
            when: (v) => !!v.report,
            rows: [
                { label: t("report.clip-out-file", "输出文件"), value: (v) => v.report?.outputFileName },
                { label: t("report.clip-out-path", "完整路径"), value: (v) => v.outPath ?? v.report?.outputFileName },
                {
                    label: t("report.clip-out-size", "体积"),
                    value: (v) =>
                        v.report
                            ? `${formatSize(v.report.outputSizeBytes)} · ${t("report.clip-file-count", "{{count}} 个文件", { count: v.report.fileCount })}`
                            : "",
                },
                { label: t("report.clip-start-jar", "启动脚本指向"), value: (v) => v.report?.startJar },
                {
                    label: t("report.clip-loader-tree", "已并入本机 loader 树"),
                    value: (v) => (v.report ? yesNo(v.report.installed) : ""),
                },
                { label: t("report.clip-root-written", "包根写入"), value: (v) => names(v.report?.generatedFiles) },
                {
                    label: t("report.clip-root-reused", "包根沿用"),
                    value: (v) =>
                        v.report?.reusedRootFiles?.length
                            ? `${names(v.report.reusedRootFiles)}${v.p.open}${t("report.clip-root-reused-note", "取自包内，本次没按配置生成")}${v.p.close}`
                            : "",
                },
                {
                    label: t("report.clip-skipped", "未打包缺件"),
                    value: (v) =>
                        v.report?.skippedMods?.length
                            ? `${names(v.report.skippedMods)}${v.p.open}${t("report.clip-skipped-note", "CurseForge 不通过接口发放下载链，已按「允许跳过」出包")}${v.p.close}`
                            : "",
                },
                {
                    label: t("report.clip-checks", "构建自检"),
                    value: (v) =>
                        v.checks.length
                            ? t("report.clip-checks-summary", "通过 {{pass}} · 警告 {{warn}} · 失败 {{fail}}", {
                                  pass: v.checks.length - v.notPassed.length,
                                  warn: v.checks.filter((c) => c.status === "warn").length,
                                  fail: v.checks.filter((c) => c.status === "fail").length,
                              })
                            : "",
                },
                {
                    line: (v) =>
                        v.notPassed
                            .map(
                                (c) =>
                                    `  [${clipCheckWord(c.status)}] ${tSource(c.label)} — ${backendText(c.detail, c.detailMsg?.key, c.detailMsg?.args)}`
                            )
                            .join("\n"),
                },
            ],
        },
        {
            title: t("report.clip-sec-error", "失败"),
            when: (v) => !!v.task.error,
            rows: [
                {
                    label: t("report.clip-error-stage", "出错阶段"),
                    value: (v) => (v.task.error ? stageLabel(v.task.error.stage) : ""),
                },
                { label: t("report.clip-error-title", "报错"), value: (v) => (v.task.error ? tSource(v.task.error.title) : "") },
                // 详情给原句：这份产物是给排查用的，URL 与退出码正是有效信息（原句没有可查的键）
                { label: t("report.clip-error-detail", "详情"), value: (v) => v.task.error?.detail },
                {
                    label: t("report.clip-retried", "已重试"),
                    value: (v) =>
                        v.task.error?.attempts
                            ? t("report.clip-times", "{{count}} 次", { count: v.task.error.attempts })
                            : "",
                },
                {
                    label: t("report.clip-retryable", "可重试"),
                    value: (v) => (v.task.error ? yesNo(v.task.error.retryable) : ""),
                },
            ],
        },
        {
            title: t("report.clip-sec-cancel", "取消"),
            when: (v) => v.task.status === "cancelled",
            rows: [
                { label: t("report.clip-stopped-at", "中断于"), value: (v) => (v.task.stage ? stageLabel(v.task.stage) : "") },
                {
                    label: t("report.clip-fetched", "已取件"),
                    value: (v) => (v.task.total ? `${v.task.downloaded ?? 0}/${v.task.total}` : ""),
                },
                {
                    label: t("report.clip-fetched-size", "已取回体积"),
                    value: (v) => (v.task.doneBytes ? formatSize(v.task.doneBytes) : ""),
                },
                {
                    label: t("report.clip-cache", "缓存"),
                    value: () => t("report.clip-cache-note", "已取回的文件保留在下载缓存，重新转换可续用"),
                },
            ],
        },
    ];
}

/**
 * 复制转换方案的正文：一次转换的可复现参数 + 本次真实结果，供详情右栏写进剪贴板。
 * 三段分工——`planClipTemplate` 是能一眼看完的八节模板，`renderClipSection` 只管滤空与排版件，
 * 这里只装配视野（`ClipView`）并把每节渲出来。
 *
 * 跟着界面语言走三档，排版件一起换：中文档的「【节】、顿号、全角括号」在英文档是
 * "[节]、逗号、半角括号"——英文句子中间挂一个顿号就是没翻完的痕迹。字段名尽量借界面上
 * 已在用的那批键（`convert.*` / `report.*` / `home.*`），同一件事在应用里只有一个叫法。
 *
 * 数据分三层：方案参数（`task.options`）是创建时钉下的快照，五种状态都在；结果（`report`）
 * 只有构建完成才有 ⇒ 失败与取消的复制文本照样给全套参数，只把【产物】一节整节省掉；
 * 逐条列名的那批模组（新增）读方案快照（`get_task_plan`），报告里只有计数。
 */
export function buildPlanSummary(
    task: ConversionTask,
    report: ConversionReport | null,
    outPath: string | null,
    plan: PlanMod[] | null
): string {
    const checks = report?.checks ?? [];
    const v: ClipView = {
        task,
        report,
        outPath,
        plan,
        o: task.options,
        p: clipPunct(),
        counts: report ? { remove: report.removed, keep: report.kept, add: report.added } : task.counts,
        checks,
        notPassed: checks.filter((c) => c.status !== "pass"),
    };
    return [`${t("report.clip-title", "SideShift 转换方案")} · v${api.APP_VERSION}`, ...planClipTemplate().map((s) => renderClipSection(s, v))]
        .filter(Boolean)
        .join("\n\n");
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
                                    {reviewHoldReason(m).trimStart() ||
                                        t("report.sides-unknown", "未判定出两端 · 请核对服务端是否需要")}
                                </span>
                            )}
                            {m.cfBlocked && m.disposition !== "remove" && (
                                <span className="text-[10px] leading-[14px] text-redstone">
                                    {t("report.skipped-mods", "拿不到文件 · 本次未打包")}
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

    if (!o.agreeEula && !holdsRootFile(report.reusedRootFiles, ROOT_EULA)) {
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
