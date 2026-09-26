/**
 * 任务引擎（浏览器 dev 兜底）：定时器模拟四阶段流水线，
 * 并预置一组历史样例让列表 / 详情 / 报告 / 错误卡都能直接走查。
 */
import type {
    CheckResult,
    ConversionOptions,
    ConversionReport,
    ConversionTask,
    PackManifest,
    PlanMod,
    StartResult,
    TaskLogLine,
    TrashEntry,
} from "@/lib/types";
import { outputNameOf, loaderLabel } from "@/lib/format";
import { hasInstallerStage } from "@/lib/rail-view";
import {
    mockDefaultOptions,
    mockManifest,
    mockPlanCounts,
    mockPlanMods,
} from "./data";
import { mockLoadSettings } from "./settings";

/** 任务内存仓（页面刷新即重置——真实实现中由 Rust 持久化） */
const tasks = new Map<string, ConversionTask>();
const timers = new Map<string, ReturnType<typeof setInterval>>();
let seq = 0;

/** 一段模拟阶段：走到 until 就换下一段，logs 是该段的样例日志 */
type StageSeg = { stage: ConversionTask["stage"]; until: number; logs: string[] };

const stagePlan: StageSeg[] = [
    { stage: "parser", until: 15, logs: ["读取清单 vault-hunters 2.4.1 · minecraft-1.20.1"] },
    {
        stage: "detector",
        until: 30,
        logs: [
            "包内内容：模组 187 个 · 其他文件 62 个 · 需联网补取 0 个",
            "方案确认：剔除 41 · 保留 146 · 新增 2",
            "剔除名单：Sodium、Iris、Xaero's Minimap…等 41 个",
        ],
    },
    {
        stage: "downloader",
        until: 82,
        logs: [
            "取件计划 146 项 · 需联网 24（≈128.0 MB）· 整合包 118 · 本地 0 · 缓存命中 4 · 并发 6",
            "复用缓存 fabric-api-0.92.2+1.20.1.jar · 1.2 MB",
            "联网获取 spark-1.10.60.jar · 2.4 MB",
            "取件 · 模组 · 118 个 · 86.0 MB",
        ],
    },
    {
        stage: "builder",
        until: 100,
        logs: [
            "生成包根文件：start.bat、start.sh、eula.txt、server.properties",
            "打包 vault-hunters-2.4.1-server.zip · 148 个文件 · 96.4 MB",
        ],
    },
];

function now(): string {
    return new Date().toTimeString().slice(0, 8);
}

/**
 * 阶段 2.5 的第一拍：把官方安装器 jar 从取件计划里拎出来单独先取（30→34）。
 * 站点仍是「下载」——本机安装不另起一站，它是下载站内部的一段。
 */
const loaderJar: StageSeg = {
    stage: "downloader",
    until: 34,
    logs: ["安装器就位：forge-1.20.1-47.2.0-installer.jar · 4.9 MB"],
};

/**
 * 「本机执行 installer」这一档模拟的是**跑一个外部进程**：分钟级、没有总量接口，
 * 所以 activity 的两个分母一律留 0（实时条按不定态脉冲），进度只按 34→42 这八个点爬。
 * 区间与真实后端同一口径；日志行给「装了多少 + 装出来的目录结构」两句，与 Rust 侧同源。
 */
const installer: StageSeg = {
    stage: "installer",
    until: 42,
    logs: [
        "本机安装 · Java 21 · 官方安装器已启动",
        "安装器 · 已写出 1328 个文件 · 106.4 MB",
        "本机安装完成 · 用时 158.2 秒 · 顶层 run.bat、run.sh",
    ],
};

/**
 * 开了「本机执行 installer」才把下载段拆成三拍（先取安装器 → 本机装 → 模组取件从 42 起脚）。
 * 拆在这里而非区间常量里，是因为 advance 按「进度落在哪一段」取样例日志与实时条口径。
 * Fabric 没有安装器可跑（它的 loader jar 是加载器本体，从版本表直取）⇒ 整档跳过。
 */
function stagePlanFor(task: ConversionTask): StageSeg[] {
    if (!hasInstallerStage(task)) return stagePlan;
    return [
        ...stagePlan.slice(0, 2),
        loaderJar,
        installer,
        ...stagePlan.slice(2),
    ];
}

/** 实时条样例文件名（轮着当「正在下载哪一个」，与 stagePlan 的下载日志同一批名字） */
const NET_SAMPLE = [
    "fabric-api-0.92.2+1.20.1.jar",
    "sodium-fabric-0.5.8+mc1.20.1.jar",
    "modmenu-7.2.2.jar",
    "spark-1.10.60.jar",
    "xaerominimap-23.9.5.jar",
];

/* ---------------- 历史样例任务 ----------------
 * 让任务列表、任务详情（失败 / 已取消）、转换报告、错误卡在浏览器 dev 下都能直接走查——
 * 只靠 mockStartTask 只能产出成功态。真实实现里这些记录由 Rust 持久化，故仅注入一次。
 */

const MIN = 60_000;

type SeedTask = Omit<ConversionTask, "options" | "counts" | "logs"> &
    Partial<Pick<ConversionTask, "options" | "counts" | "logs">>;

function seedTask(p: SeedTask): ConversionTask {
    return {
        ...p,
        options: p.options ?? mockDefaultOptions,
        counts: p.counts ?? mockPlanCounts,
        logs: p.logs ?? [],
    };
}

const HISTORY: ConversionTask[] = [
    seedTask({
        id: "seed-failed",
        pack: { ...mockManifest, fileName: "medieval-world-1.3.zip", loader: "forge", mcVersion: "1.16.5", modCount: 74, sizeBytes: 210 * 1024 * 1024 },
        options: { ...mockDefaultOptions, mcVersion: "1.16.5", loaderVersion: "36.2.39", javaVersion: "17" },
        status: "failed",
        stage: "downloader",
        progress: 46,
        downloaded: 34,
        total: 74,
        fetch: {
            files: 74,
            bytes: 208_000_000,
            netFiles: 40,
            netBytes: 164_000_000,
            packFiles: 32,
            localFiles: 0,
            cachedFiles: 2,
        },
        netDone: 20,
        doneBytes: 96_000_000,
        counts: { remove: 18, keep: 52, add: 4 },
        createdAt: Date.now() - 42 * MIN,
        startedAt: Date.now() - 42 * MIN,
        finishedAt: Date.now() - 37 * MIN,
        error: {
            stage: "downloader",
            title: "forge-installer 退出码 1",
            detail: "下载 maven.minecraftforge.net 超时（已重试 3 次），可在设置中切换镜像源",
            retryable: true,
            attempts: 3,
        },
        logs: [
            { time: "14:02:11", stage: "parser", message: "读取清单 medieval-world 1.3 · minecraft-1.16.5", level: "info" },
            { time: "14:02:19", stage: "detector", message: "发现 forge-36.2.39 · 74 个模组 · 18 个客户端专属已标记剔除", level: "info" },
            { time: "14:07:03", stage: "downloader", message: "forge-installer.jar 下载超时 · 第 3 次重试失败", level: "error" },
        ],
    }),
    seedTask({
        id: "seed-success-2",
        pack: { ...mockManifest, fileName: "create-fabric-1.21.4.mrpack", mcVersion: "1.21.4", modCount: 96, sizeBytes: 412 * 1024 * 1024 },
        options: { ...mockDefaultOptions, mcVersion: "1.21.4", loaderVersion: "0.16.9" },
        status: "success",
        stage: "builder",
        progress: 100,
        counts: { remove: 22, keep: 71, add: 3 },
        createdAt: Date.now() - 3 * 60 * MIN,
        startedAt: Date.now() - 3 * 60 * MIN,
        finishedAt: Date.now() - 3 * 60 * MIN + 5 * MIN + 8_000,
        outputFileName: "create-server-1.21.4.zip",
        outputSizeBytes: 412 * 1024 * 1024,
        logs: [
            { time: "11:20:41", stage: "parser", message: "读取清单 create-fabric 1.21.4 · minecraft-1.21.4", level: "info" },
            { time: "11:25:02", stage: "builder", message: "生成 start.sh / start.bat · 打包 create-server-1.21.4.zip", level: "info" },
        ],
    }),
    seedTask({
        id: "seed-cancelled",
        pack: { ...mockManifest, fileName: "all-the-mods-9-1.0.2.mrpack", modCount: 342, sizeBytes: 690 * 1024 * 1024 },
        status: "cancelled",
        stage: "downloader",
        progress: 38,
        downloaded: 130,
        total: 342,
        fetch: {
            files: 342,
            bytes: 690_000_000,
            netFiles: 96,
            netBytes: 402_000_000,
            packFiles: 240,
            localFiles: 0,
            cachedFiles: 6,
        },
        netDone: 44,
        doneBytes: 268_000_000,
        counts: { remove: 96, keep: 240, add: 6 },
        createdAt: Date.now() - 26 * 60 * MIN,
        startedAt: Date.now() - 26 * 60 * MIN,
        finishedAt: Date.now() - 22 * 60 * MIN,
        logs: [
            { time: "13:41:08", stage: "detector", message: "发现 fabric-loader 0.15.3 · 342 个模组 · 96 个客户端专属已标记剔除", level: "info" },
            { time: "13:45:52", stage: "downloader", message: "任务已被用户取消", level: "warn" },
        ],
    }),
    seedTask({
        id: "seed-success-1",
        pack: { ...mockManifest, fileName: "fabulously-optimized-7.1.mrpack", modCount: 214, sizeBytes: 188 * 1024 * 1024 },
        status: "success",
        stage: "builder",
        progress: 100,
        counts: { remove: 63, keep: 123, add: 28 },
        createdAt: Date.now() - 5 * 60 * MIN,
        startedAt: Date.now() - 5 * 60 * MIN,
        finishedAt: Date.now() - 5 * 60 * MIN + 3 * MIN + 42_000,
        outputFileName: "fo-7.1-server.zip",
        outputSizeBytes: 236 * 1024 * 1024,
        logs: [
            { time: "09:12:30", stage: "parser", message: "读取清单 fabulously-optimized 7.1 · minecraft-1.20.1", level: "info" },
            { time: "09:16:12", stage: "builder", message: "生成 start.sh / start.bat · 打包 fo-7.1-server.zip", level: "info" },
        ],
    }),
];

let seeded = false;

/** 首次访问任务仓时注入历史样例（之后由用户操作驱动，删完不会再冒出来） */
function ensureSeeded(): void {
    if (seeded) return;
    seeded = true;
    for (const t of HISTORY) tasks.set(t.id, t);
    seq = HISTORY.length;
}

/** 启动（或重启）一个模拟转换任务；同一时间只跑一条，有任务在跑则排队 */
export function mockStartTask(
    options: ConversionOptions,
    pack: PackManifest = mockManifest
): StartResult {
    ensureSeeded();
    const queued = [...tasks.values()].some((t) => t.status === "running");
    const id = `task-${++seq}`;
    const task: ConversionTask = {
        id,
        pack,
        options,
        status: queued ? "queued" : "running",
        progress: 0,
        counts: mockPlanCounts,
        createdAt: Date.now(),
        startedAt: queued ? undefined : Date.now(),
        logs: [],
    };
    tasks.set(id, task);
    if (!queued) advance(id);
    return { taskId: id, queued };
}

/** 队首转正：当前无运行时，把最早创建的排队任务拉起（与 Rust release_and_next 同语义） */
function dequeueNext(): void {
    if ([...tasks.values()].some((t) => t.status === "running")) return;
    const next = [...tasks.values()]
        .filter((t) => t.status === "queued")
        .sort((a, b) => a.createdAt - b.createdAt)[0];
    if (!next) return;
    next.status = "running";
    next.startedAt = Date.now();
    advance(next.id);
}

function advance(id: string) {
    const timer = setInterval(() => {
        const task = tasks.get(id);
        if (!task || task.status !== "running") {
            clearInterval(timer);
            timers.delete(id);
            dequeueNext();
            return;
        }
        task.progress = Math.min(100, task.progress + 2);
        const plan = stagePlanFor(task);
        const segIdx = plan.findIndex((s) => task.progress <= s.until);
        const seg = plan[segIdx];
        task.stage = seg.stage;
        if (seg.stage === "downloader") {
            // 取件构成：与真实后端同一形态（146 项里只有 24 项真联网）
            task.fetch ??= {
                files: 146,
                bytes: 412_000_000,
                netFiles: 24,
                netBytes: 128_000_000,
                packFiles: 118,
                localFiles: 0,
                cachedFiles: 4,
            };
            const ratio = task.progress / 100;
            task.downloaded = Math.round(ratio * 146);
            task.total = 146;
            task.netDone = Math.round(ratio * task.fetch.netFiles);
            task.doneBytes = Math.round(ratio * task.fetch.bytes);
            // 实时条走联网口径（缓存/本地件零流量，计进来会让条跑得比真实网络快）
            task.activity = {
                kind: "net",
                subject: NET_SAMPLE[(task.netDone ?? 0) % NET_SAMPLE.length],
                doneBytes: Math.round(ratio * task.fetch.netBytes),
                totalBytes: task.fetch.netBytes,
                itemsDone: task.netDone ?? 0,
                itemsTotal: task.fetch.netFiles,
                rateBps: 2_400_000,
                attempt: 1,
            };
        } else if (seg.stage === "installer") {
            // 安装器那边只有「已经写出多少」，没有总量接口 ⇒ 两个分母留 0，实时条按不定态脉冲
            const from = plan[segIdx - 1]?.until ?? 0;
            const p = (task.progress - from) / (seg.until - from);
            task.activity = {
                kind: "install",
                subject: `${loaderLabel(task.pack.loader)} ${task.options.mcVersion}-${task.options.loaderVersion}`,
                doneBytes: Math.round(p * 106_400_000),
                totalBytes: 0,
                itemsDone: Math.round(p * 1328),
                itemsTotal: 0,
                rateBps: 860_000,
                attempt: 1,
            };
        } else if (seg.stage === "builder") {
            // 打包段：开了本机安装时基座从 92 起算，没开仍是 82→100（分母用产物体积，subject 随已写字节换目录）
            const from = plan[segIdx - 1]?.until ?? 0;
            const p = (task.progress - from) / (seg.until - from);
            const bytes = 101_187_000;
            task.activity = {
                kind: "zip",
                subject: p < 0.82 ? "模组" : p < 0.94 ? "config" : "根文件",
                doneBytes: Math.round(p * bytes),
                totalBytes: bytes,
                itemsDone: Math.round(p * 148),
                itemsTotal: 148,
                rateBps: 64_000_000,
                attempt: 1,
            };
        } else {
            task.activity = undefined;
        }
        // 每进入新阶段补一条日志（近似：按进度里程碑）
        if (task.progress % 15 === 2) {
            const line: TaskLogLine = {
                time: now(),
                stage: seg.stage!,
                message: seg.logs[Math.floor(Math.random() * seg.logs.length)],
                level: "info",
            };
            task.logs.push(line);
        }
        if (task.progress >= 100) {
            task.status = "success";
            task.activity = undefined;
            task.finishedAt = Date.now();
            task.outputFileName = outputNameOf(task.pack.fileName);
            task.outputSizeBytes = 96 * 1024 * 1024;
            clearInterval(timer);
            timers.delete(id);
            dequeueNext();
        }
    }, 200);
    timers.set(id, timer);
}

export function mockListTasks(): ConversionTask[] {
    ensureSeeded();
    return [...tasks.values()].sort((a, b) => b.createdAt - a.createdAt);
}

export function mockGetTask(id: string): ConversionTask | undefined {
    ensureSeeded();
    return tasks.get(id);
}

export function mockCancelTask(id: string): void {
    const task = tasks.get(id);
    if (task && (task.status === "running" || task.status === "queued")) {
        const wasQueued = task.status === "queued";
        task.status = "cancelled";
        task.activity = undefined;
        task.finishedAt = Date.now();
        task.logs.push({ time: now(), stage: task.stage ?? "builder", message: "任务已被用户取消", level: "warn" });
        // 排队行没有推进器，取消后由其替运行中任务交棒；运行中的交棒在 advance 里做
        if (wasQueued) dequeueNext();
    }
}

export function mockRetryTask(id: string): StartResult | undefined {
    const task = tasks.get(id);
    if (!task) return undefined;
    return mockStartTask(task.options, task.pack);
}

/** 回收站（与 Rust 侧同构：只活在内存，刷新页面即清空），撤回要把任务原样放回 */
const trash = new Map<string, { task: ConversionTask; deletedAt: number }>();

/** 删除任务记录 = 搬进回收站（终态任务才可删；运行中先取消） */
export function mockDeleteTask(id: string): void {
    const task = tasks.get(id);
    if (!task) return;
    const timer = timers.get(id);
    if (timer) clearInterval(timer);
    timers.delete(id);
    tasks.delete(id);
    trash.set(id, { task, deletedAt: Date.now() });
    dequeueNext();
}

function toTrashEntry(id: string, t: ConversionTask, deletedAt: number): TrashEntry {
    return {
        taskId: id,
        packFileName: t.pack.fileName,
        loader: t.pack.loader,
        mcVersion: t.options.mcVersion,
        status: t.status,
        outputFileName: t.outputFileName,
        outputSizeBytes: t.outputSizeBytes,
        deletedAt,
    };
}

export function mockListTrash(): TrashEntry[] {
    return [...trash.entries()]
        .map(([id, e]) => toTrashEntry(id, e.task, e.deletedAt))
        .sort((a, b) => b.deletedAt - a.deletedAt);
}

/** 撤回删除：任务回到列表（与 Rust 一样只放回内存仓，不做状态校验） */
export function mockRestoreTask(id: string): void {
    const e = trash.get(id);
    if (!e) return;
    trash.delete(id);
    tasks.set(id, e.task);
}

/** 清空回收站，返回丢弃条数 */
export function mockClearTrash(): number {
    const n = trash.size;
    trash.clear();
    return n;
}

/** 镜像 Rust 的 `msg!` + `check()`：一条模板句同时给出「渲染好的中文整句」和「模板 + 参数」。
 *  两边口径必须一致——mock 里只给整句的话，界面上那条永远翻不出来，真机却翻得出来，
 *  自测就会在两种数据下看到两套语言。 */
function msg(key: string, args?: Record<string, unknown>) {
    const zh = key.replace(/\{\{\s*(\w+)\s*\}\}/g, (_, slot: string) =>
        String(args?.[slot] ?? `{{${slot}}}`)
    );
    return { detail: zh, detailMsg: { key, args: args ?? null, zh } };
}

export function mockReport(taskId: string): ConversionReport | undefined {
    ensureSeeded();
    const task = tasks.get(taskId);
    if (!task) return undefined;
    const o = task.options;
    const generated = ["eula.txt", "server.properties", "README-SideShift.txt"];
    if (o.generateScripts) generated.unshift("start.bat", "start.sh");
    const mods = (task.counts?.keep ?? mockPlanCounts.keep) + (task.counts?.add ?? mockPlanCounts.add);
    // 自检明细只在开关打开时给（与 Rust 侧一致：关着不该凭空冒出一张卡）
    const checks: CheckResult[] = mockLoadSettings().verifyAfterBuild
        ? [
              { id: "files", label: "取件完整", status: "pass", ...msg("模组 {{count}} 个全部落位", { count: mods }) },
              { id: "jars", label: "jar 可用", status: "pass", ...msg("{{count}} 个 jar 容器可读", { count: mods + 1 }) },
              { id: "deps", label: "依赖闭合", status: "pass", ...msg("{{count}} 条依赖引用全部指向包内", { count: 12 }) },
              o.generateScripts
                  ? {
                        id: "start",
                        label: "启动指向",
                        status: "pass",
                        ...msg(
                            "{{jar}} 就位 · 首次运行会联网装出 loader（需本机 Java 与网络）",
                            { jar: "fabric-server-launch.jar" }
                        ),
                    }
                  : {
                        id: "start",
                        label: "启动指向",
                        status: "warn",
                        ...msg(
                            "{{jar}} 就位；本次未生成启动脚本，需自行按包内文件启动",
                            { jar: "fabric-server-launch.jar" }
                        ),
                    },
              // 假数据走的是「没本机安装」那一档：加载器要首启现装，所以这一行是提示档而非通过档
              {
                  id: "loader",
                  label: "Loader 就位",
                  status: "warn",
                  ...msg("只有 {{jar}}·首次运行才联网装出加载器", {
                      jar: "fabric-server-launch.jar",
                  }),
              },
              { id: "root", label: "包根文件", status: "pass", ...msg("包根 {{count}} 个文件全部就位", { count: generated.length }) },
              ...(o.keepDirs.length
                  ? [
                        {
                            id: "keep",
                            label: "保留目录",
                            status: "pass" as const,
                            ...msg("{{dirs}} 个目录 · {{files}} 个文件已带入", {
                                dirs: o.keepDirs.length,
                                files: 12,
                            }),
                        },
                    ]
                  : []),
          ]
        : [];
    return {
        taskId,
        outputFileName: task.outputFileName ?? outputNameOf(task.pack.fileName),
        outputSizeBytes: task.outputSizeBytes ?? 0,
        durationSec: Math.round(((task.finishedAt ?? Date.now()) - (task.startedAt ?? Date.now())) / 1000),
        removed: task.counts?.remove ?? mockPlanCounts.remove,
        kept: task.counts?.keep ?? mockPlanCounts.keep,
        added: task.counts?.add ?? mockPlanCounts.add,
        pendingReview: ["ViaFabricPlus"],
        options: task.options,
        // 演示口径：包根文件 + mods/config 两个目录的条目数
        fileCount: (task.counts?.keep ?? mockPlanCounts.keep) + (task.counts?.add ?? mockPlanCounts.add) + generated.length + 12,
        generatedFiles: generated,
        startJar: "fabric-server-launch.jar",
        // 演示包是 Fabric：它没有安装器 jar 可在本机跑
        installed: false,
        checks,
    };
}

/** 任务方案快照：按该行任务的计数截取，好让展开清单与报告数字对得上 */
export function mockGetTaskPlan(taskId: string): PlanMod[] {
    ensureSeeded();
    const c = tasks.get(taskId)?.counts ?? mockPlanCounts;
    const take = (d: PlanMod["disposition"], n: number) =>
        mockPlanMods.filter((m) => m.disposition === d).slice(0, n);
    return [...take("remove", c.remove), ...take("keep", c.keep), ...take("add", c.add)];
}
