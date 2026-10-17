/**
 * 本机开服验证的浏览器 dev 兜底：一份会话内存仓 + 一拍一走完的六步定时器。
 * 每次「开始验证」按 `OUTCOMES` 出结果（通过 → 失败 → 超时 → 不可判定 → 交付包不在），
 * 目的是在这台机器上不碰真服务端也能把页面的每一档走查一遍；真机一律以后端为准。
 * 副本与模组清单都是假数据：这里的 `copyDir` 打开不了，启停只改内存里那一份表。
 */
import type {
    ModToggleResult,
    VerifyLogLine,
    VerifyModEntry,
    VerifySession,
    VerifySource,
    VerifyStatus,
    VerifyStep,
    VerifyStepId,
} from "@/lib/types";
import { backendMsg, logMsg, noteMsg } from "./msg";

/** 假路径只用来在界面上显示与走查「打开目录」这条调用链，真机上由后端给绝对路径 */
const MOCK_CACHE = "C:\\Users\\dev\\AppData\\Roaming\\SideShift\\cache";

/** 日志环预算：与后端 `events::MAX_LOG_LINES` 同一档，页面上的「共 N 条」按它算 */
const RING = 600;

const STEP_DEFS: [VerifyStepId, string][] = [
    ["env", "检查本机环境"],
    ["unpack", "解包交付包"],
    ["reconcile", "产物对账"],
    ["loader", "首启装加载器"],
    ["launch", "启动服务端进程"],
    ["wait", "等待 Done"],
];

/** 样例模组：够让启停弹窗把两档、搜索与汇总都走查一遍 */
const MOD_SAMPLE: [string, number][] = [
    ["fabric-api-0.92.2+1.20.1.jar", 1_258_291],
    ["fabric-permissions-api-0.2.SNAPSHOT.jar", 8_192],
    ["sodium-fabric-0.5.8+mc1.20.1.jar", 912_400],
    ["modmenu-7.2.2.jar", 342_010],
    ["krypton-0.2.3.jar", 45_786],
    ["spark-1.10.60.jar", 5_242_880],
    ["xaerominimap-23.9.5.jar", 2_411_724],
    ["cloth-config-11.1.136-fabric.jar", 612_340],
    ["architectury-9.2.14-fabric.jar", 1_048_576],
    ["memory-leak-fix-1.0.0.jar", 12_288],
    ["vault-hunters-compat-1.2.jar", 30_720],
    ["appleskin-fabric-mc1.20-2.5.1.jar", 218_112],
];

function seedMods(taskId: string): VerifyModEntry[] {
    let list = mods.get(taskId);
    if (!list) {
        list = MOD_SAMPLE.map(([file, sizeBytes]) => ({
            file,
            name: file,
            enabled: file !== "memory-leak-fix-1.0.0.jar",
            sizeBytes,
        }));
        mods.set(taskId, list);
    }
    return list;
}

function counts(list: VerifyModEntry[]): [number, number] {
    return [list.length, list.filter((m) => !m.enabled).length];
}

function step(id: VerifyStepId, label: string): VerifyStep {
    return { id, label, status: "pending", note: "", noteMsg: null };
}

function emptySession(taskId: string): VerifySession {
    return {
        taskId,
        status: "ready",
        steps: [],
        logs: [],
        copyDir: "",
        copyBytes: 0,
        modTotal: 0,
        modDisabled: 0,
        startedAt: null,
        finishedAt: null,
        elapsedSec: 0,
        exitCode: null,
        serverPort: null,
        logLines: 0,
        warnings: [],
        failure: null,
        blockers: [],
        hints: [],
        jvmXmx: "-Xmx4096M",
        onlineMode: false,
        timeoutSec: 30 * 60,
        loaderInstalled: false,
    };
}

function now(): string {
    return new Date().toTimeString().slice(0, 8);
}

function push(s: VerifySession, source: VerifySource, level: VerifyLogLine["level"], key: string, args?: Record<string, unknown>) {
    const m = logMsg(key, args);
    s.logs.push({ time: now(), source, level, message: m.message, messageMsg: m.messageMsg });
    if (s.logs.length > RING) s.logs.shift();
    s.logLines += 1;
}

/** 一拍：把某一步换态并补几句它的输出 */
interface Beat {
    step: VerifyStepId;
    status: VerifyStep["status"];
    note?: string;
    noteArgs?: Record<string, unknown>;
    lines: [VerifySource, VerifyLogLine["level"], string, Record<string, unknown>?][];
}

function applyBeat(s: VerifySession, b: Beat) {
    const st = s.steps.find((x) => x.id === b.step);
    if (st) {
        Object.assign(st, b.note ? noteMsg(b.note, b.noteArgs) : { note: "", noteMsg: null }, {
            status: b.status,
        });
    }
    for (const [source, level, key, args] of b.lines) push(s, source, level, key, args);
    // 解包这一步落定才有副本：目录、大小与模组计数都从这一刻起有数
    if (b.step === "unpack" && b.status !== "pending") {
        const list = seedMods(s.taskId);
        s.copyDir = `${MOCK_CACHE}\\verify\\${s.taskId}`;
        [s.modTotal, s.modDisabled] = counts(list);
        s.copyBytes = list.reduce((n, m) => n + m.sizeBytes, 0) + 104_857_600;
    }
}

/** 六步的共用前缀（通过/失败/超时三档都走到「等待 Done」之前） */
function commonBeats(s: VerifySession): Beat[] {
    const port = 41_223;
    return [
        {
            step: "env",
            status: "pass",
            note: "本次使用 Java {{have}}：{{path}}",
            noteArgs: { have: "17", path: "C:\\Java\\jdk-17\\bin\\java.exe" },
            lines: [
                ["env", "info", "本次使用 Java {{have}}：{{path}}", { have: "17", path: "C:\\Java\\jdk-17\\bin\\java.exe" }],
                ["env", "warn", "交付包里的 eula.txt 没有同意最终用户协议，服务端会拒绝启动（验证不代填这一项）"],
            ],
        },
        {
            step: "unpack",
            status: "pass",
            note: "{{files}} 个文件 · {{bytes}} 字节",
            noteArgs: { files: 160, bytes: 121_634_837 },
            lines: [
                ["unpack", "info", "副本就位：{{dir}} · {{files}} 个文件", { dir: `${MOCK_CACHE}\\verify\\${s.taskId}`, files: 160 }],
                ["unpack", "warn", "mods 目录共 {{total}} 枚模组，其中 {{disabled}} 枚处于停用状态", { total: s.modTotal, disabled: s.modDisabled }],
            ],
        },
        {
            step: "reconcile",
            status: "pass",
            note: "构建自检 {{passed}} 项全部通过",
            noteArgs: { passed: 6 },
            lines: [["verify", "info", "构建自检 {{passed}} 项全部通过", { passed: 6 }]],
        },
        {
            step: "loader",
            status: "skipped",
            note: "Fabric 的加载器随服务端首启自装，这一步不单独跑安装器",
            lines: [["loader", "info", "Fabric 的加载器随服务端首启自装，这一步不单独跑安装器"]],
        },
        {
            step: "launch",
            status: "pass",
            note: "进程已起 · {{script}}",
            noteArgs: { script: "start.bat" },
            lines: [
                ["env", "warn", "端口 {{from}} 已被本机其它程序占用，本次改用空闲端口 {{to}}：新值只写进副本的 server.properties，交付包不动", { from: 25565, to: port }],
                ["java", "info", "按 {{script}} 起进程：{{command}}", { script: "start.bat", command: "C:\\Java\\jdk-17\\bin\\java.exe -Xmx4096M -jar fabric-server-launch.jar nogui" }],
                ["server", "info", "[14:02:11] [main/INFO] [fabric-loader] Loading 12 mods"],
            ],
        },
    ];
}

/** 通过档：一堆原文输出 + 那句就绪判据 */
const PASS_BEATS: Beat[] = [
    {
        step: "wait",
        status: "pass",
        note: "服务端已就绪（{{sec}} 秒）",
        noteArgs: { sec: 72 },
        lines: [
            ["server", "info", "[14:02:19] [Server thread/INFO]: Starting minecraft server version 1.20.1"],
            ["server", "info", "[14:02:31] [Server thread/INFO]: Preparing start region for dimension minecraft:overworld"],
            ["server", "warn", "[14:03:02] [Server thread/WARN]: Can't keep up! Is the server overloaded? Running 5234ms or 104 ticks behind"],
            ["server", "info", "[14:03:23] [Server thread/INFO]: Done (61.42s)! For help, type \"help\""],
            ["server", "info", "服务端已就绪（72 秒）"],
        ],
    },
];

/** 失败档：Mixin 报错 + 进程自己退出 */
const FAIL_BEATS: Beat[] = [
    {
        step: "wait",
        status: "fail",
        note: "进程退出（{{code}}），没有等到就绪",
        noteArgs: { code: 1 },
        lines: [
            ["server", "info", "[14:02:19] [Server thread/INFO] [mixin]: Preparing mixin metadata from krypton.mixins.json"],
            ["mixin", "error", "[14:02:26] [main/ERROR] [mixin]: Mixin apply failed krypton.mixins.json:server.MixinServerGamePacketListenerImpl -> net.minecraft.class_3244"],
            ["mixin", "error", "[14:02:26] [main/ERROR] [mixin]: org.spongepowered.asm.mixin.transformer.throwables.MixinTransformerError: An unexpected critical error was encountered"],
            ["server", "error", "[14:02:27] [Server thread/ERROR]: Encountered an unexpected exception"],
            ["server", "info", "进程退出（1），没有等到就绪"],
        ],
    },
];

/** 超时档：停在生成世界那一步，等到内置上限 */
const TIMEOUT_BEATS: Beat[] = [
    {
        step: "wait",
        status: "warn",
        note: "{{secs}} 秒内没有等到就绪，进程已终止（不判包坏）",
        noteArgs: { secs: 30 * 60 },
        lines: [
            ["server", "info", "[14:02:19] [Server thread/INFO]: Preparing spawn area: 12%"],
            ["server", "info", "[14:31:58] [Server thread/INFO]: Preparing spawn area: 76%"],
            ["server", "warn", "1800 秒内没有等到就绪，进程已终止（不判包坏）"],
        ],
    },
];

/** 不可判定档：环境那一步就断，后面五步一律不执行 */
const BLOCKED_BEAT: Beat = {
    step: "env",
    status: "fail",
    note: "本机没有满足需求的 Java：需要 {{want}} 或更高，本次找到 {{have}}",
    noteArgs: { want: "17", have: "8" },
    lines: [["env", "error", "未检测到可用 JDK · 本次需要 Java 17 或更高"]],
};

/** 交付包不在：同样断在第一步，但说的是那口包 */
const MISSING_BEAT: Beat = {
    step: "env",
    status: "fail",
    note: "交付包不在了：{{path}}",
    noteArgs: { path: `${MOCK_CACHE}\\..\\output\\vault-hunters-2.4.1-server.zip` },
    lines: [["env", "error", "输出目录中未找到 vault-hunters-2.4.1-server.zip · 六步未执行"]],
};

/** 六步一次摆齐（与后端 `new_steps` 同形） */
function freshSteps(): VerifyStep[] {
    return STEP_DEFS.map(([id, label]) => step(id, label));
}

function settle(s: VerifySession, outcome: VerifyStatus) {
    s.status = outcome;
    s.finishedAt = Date.now();
    s.elapsedSec = Math.round((s.finishedAt - (s.startedAt ?? s.finishedAt)) / 1000);
    if (outcome === "passed") {
        s.exitCode = 0;
        s.serverPort = 41_223;
        s.warnings = [
            { line: 12, text: "Can't keep up! Is the server overloaded? Running 5234ms or 104 ticks behind" },
        ];
    } else if (outcome === "failed") {
        s.exitCode = 1;
        s.serverPort = 41_223;
        s.failure = {
            step: "wait",
            exitCode: 1,
            jars: ["krypton-0.2.3.jar"],
            lines: [
                { line: 388, text: "Mixin apply failed krypton.mixins.json:server.MixinServerGamePacketListenerImpl -> net.minecraft.class_3244" },
                { line: 389, text: "org.spongepowered.asm.mixin.transformer.throwables.MixinTransformerError: An unexpected critical error was encountered" },
            ],
        };
        // eula 那一句既在日志里、也摊进失败定位（后端 `hints` 的同款）
        s.hints = [backendMsg("交付包里的 eula.txt 没有同意最终用户协议，服务端会拒绝启动（验证不代填这一项）")];
    } else if (outcome === "timeout") {
        s.serverPort = 41_223;
    } else if (outcome === "blocked") {
        s.blockers = [backendMsg("未检测到可用 JDK · 本次需要 Java 17 或更高")];
    }
}

const OUTCOMES: VerifyStatus[] = ["passed", "failed", "timeout", "blocked", "missing"];
let outcomeSeq = 0;

const sessions = new Map<string, VerifySession>();
const timers = new Map<string, ReturnType<typeof setInterval>>();
const mods = new Map<string, VerifyModEntry[]>();

/** 会话快照：没跑过就回默认那一档（与后端 `VerifySession::ready` 同形） */
export function mockVerifyState(id: string): VerifySession {
    return sessions.get(id) ?? emptySession(id);
}

/** 开一轮：六步一次摆齐，之后每拍推进一步 */
export function mockVerifyStart(id: string): void {
    const s = emptySession(id);
    s.status = "running";
    s.steps = freshSteps();
    s.startedAt = Date.now();
    sessions.set(id, s);
    const outcome = OUTCOMES[outcomeSeq++ % OUTCOMES.length];
    const beats: Beat[] =
        outcome === "blocked"
            ? [BLOCKED_BEAT]
            : outcome === "missing"
              ? [MISSING_BEAT]
              : [
                    ...commonBeats(s),
                    ...(outcome === "passed" ? PASS_BEATS : outcome === "failed" ? FAIL_BEATS : TIMEOUT_BEATS),
                ];
    let beat = 0;
    s.steps[0].status = "running";
    const timer = setInterval(() => {
        const cur = sessions.get(id);
        if (!cur || cur.status !== "running") {
            clearInterval(timer);
            timers.delete(id);
            return;
        }
        const b = beats[beat];
        if (!b) {
            // 拍子走完：这一档的结论、退出码、警告由 settle 补上
            clearInterval(timer);
            timers.delete(id);
            settle(cur, outcome);
            return;
        }
        applyBeat(cur, b);
        beat += 1;
        const next = cur.steps.find((x) => x.id === beats[beat]?.step);
        if (next) next.status = "running";
    }, 1200);
    timers.set(id, timer);
}

/** 取消：杀掉定时器，副本与日志都留着（口径与后端一致——清理由用户点「清理验证目录」） */
export function mockVerifyCancel(id: string): boolean {
    const timer = timers.get(id);
    const s = sessions.get(id);
    if (!timer || !s || s.status !== "running") return false;
    clearInterval(timer);
    timers.delete(id);
    push(s, "env", "info", "已请求取消，服务端进程会被终止");
    push(s, "server", "warn", "已取消：服务端进程已终止，本次不作判定");
    s.status = "cancelled";
    s.finishedAt = Date.now();
    s.elapsedSec = Math.round((s.finishedAt - (s.startedAt ?? s.finishedAt)) / 1000);
    return true;
}

export function mockVerifyMods(id: string): VerifyModEntry[] {
    return seedMods(id);
}

export function mockVerifySetMod(id: string, file: string, enabled: boolean): ModToggleResult {
    const list = seedMods(id);
    const hit = list.find((m) => m.file === file);
    if (!hit) throw new Error(`「${file}」不是 mods 目录里的模组文件，没有改动`);
    hit.enabled = enabled;
    hit.file = enabled ? file.replace(/\.disabled$/, "") : `${file}.disabled`;
    hit.name = hit.file;
    const [modTotal, modDisabled] = counts(list);
    const s = sessions.get(id);
    if (s) {
        s.modTotal = modTotal;
        s.modDisabled = modDisabled;
    }
    return { entry: { ...hit }, mods: list.map((m) => ({ ...m })), modTotal, modDisabled };
}

/** 清理验证目录：副本没了，会话回到默认那一档 */
export function mockVerifyClean(id: string): void {
    const timer = timers.get(id);
    if (timer) clearInterval(timer);
    timers.delete(id);
    sessions.delete(id);
    mods.delete(id);
}
