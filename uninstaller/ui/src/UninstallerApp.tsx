/**
 * 卸载壳的主组件：四态一屏（确认 / 卸载中 / 已完成 / 未完成）
 *
 * 状态机只有 page 一轴，和安装壳同构。两件事刻意不做：
 * - 没有「退出并继续」这颗钮：静默卸载走的是官方 NSIS 卸载器，它自己会按 exe 名结束在跑的
 *   SideShift（`IfSilent` → `KillProcess`）。壳再补一次 TerminateProcess 只是把同一个动作
 *   提前，还把「正在转换的整合包写了一半」这个风险变成了壳的责任。所以这里只报事实、只轮询，
 *   按钮一直可用；用户自己退了应用，卡片当场消失。
 * - 没有阶段清单：卸载只有"删文件"一件事，进度条的分母是实测还剩多少字节，多列几行只能靠猜。
 *
 * `valid` 为假（壳被单独拷出来跑、安装目录已被手删）时正文只有那张卡，页脚只留「关闭」：
 * 这时候任何"卸载"动作都没有对象。
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Btn } from "@/components/ui/Button";
import { cn } from "@/lib/utils";
import {
    checkRunning,
    getSnapshot,
    onProgress,
    openPath,
    runUninstall,
    type Outcome,
    type Progress,
    type Snapshot,
} from "./api";
import { TitleBar } from "./TitleBar";

type Page = "confirm" | "busy" | "done" | "fail";

/** 应用运行中轮询的节拍：够快到人关掉窗口后按钮就放开，又不至于让 WebView 一直忙 */
const POLL_MS = 900;

export function UninstallerApp() {
    const [snap, setSnap] = useState<Snapshot | null>(null);
    const [snapErr, setSnapErr] = useState<string | null>(null);
    const [running, setRunning] = useState(false);

    const [page, setPage] = useState<Page>("confirm");
    const [progress, setProgress] = useState<Progress | null>(null);
    const [error, setError] = useState<string | null>(null);
    const [outcome, setOutcome] = useState<Outcome | null>(null);
    const [pathErr, setPathErr] = useState<string | null>(null);

    const alive = useRef(true);
    useEffect(() => {
        alive.current = true;
        getSnapshot()
            .then((s) => {
                if (!alive.current) return;
                setSnap(s);
                setRunning(s.running);
            })
            .catch((e) => alive.current && setSnapErr(String(e)));
        return () => {
            alive.current = false;
        };
    }, []);

    // 只在确认页轮询：进了卸载页，应用是不是还在跑已经不是这个界面要回答的问题
    useEffect(() => {
        if (page !== "confirm" || snap === null || !snap.valid) return;
        const id = setInterval(() => {
            checkRunning()
                .then((v) => alive.current && setRunning(v))
                .catch(() => {});
        }, POLL_MS);
        return () => clearInterval(id);
    }, [page, snap?.valid]);

    useEffect(() => {
        const unlisten = onProgress((p) => alive.current && setProgress(p));
        return () => void unlisten.then((f) => f());
    }, []);

    const start = useCallback(async () => {
        setProgress(null);
        setError(null);
        setOutcome(null);
        setPage("busy");
        try {
            setOutcome(await runUninstall());
            setPage("done");
        } catch (e) {
            setError(String(e));
            setPage("fail");
        }
    }, []);

    const back = useCallback(() => {
        setError(null);
        setProgress(null);
        setPage("confirm");
    }, []);

    const close = () => void getCurrentWindow().close();

    const browse = (path: string) => {
        // 卸载完之后目录还在不在是另一件事，打不开就得说清楚：只留提示，不改已完成这页的结论
        setPathErr(null);
        openPath(path).catch((e) => alive.current && setPathErr(String(e)));
    };

    // 快照都拿不到（get_snapshot 出错）与"安装目录不在"是同一态：界面上没有可卸载的对象
    const blocked = (snap !== null && !snap.valid) || snapErr !== null;

    return (
        <div className="flex h-screen flex-col overflow-hidden bg-bg-app text-text-1">
            <TitleBar
                meta={snap ? `${snap.version} · ${snap.arch}` : null}
            />

            <main className="page-scroll min-h-0 flex-1 overflow-y-auto px-7 pt-[26px] pb-5">
                {blocked ? (
                    <Card
                        tone="bad"
                        title="找不到安装位置"
                        body={
                            snapErr ??
                            "这个卸载程序必须和 SideShift.exe 装在同一个目录里。要卸载请从原安装目录运行，或重新安装后再试。"
                        }
                    />
                ) : (
                    <>
                        {(page === "confirm" || page === "busy" || page === "fail") && (
                            <ConfirmBody
                                page={page}
                                running={running}
                                progress={progress}
                                error={error}
                            />
                        )}
                        {page === "done" && outcome && (
                            <DoneBody outcome={outcome} onOpen={browse} error={pathErr} />
                        )}
                    </>
                )}
            </main>

            <footer className="flex shrink-0 items-center gap-2 border-t border-stroke bg-bg-panel px-7 py-3">
                {page === "confirm" && !blocked && (
                    <>
                        <Btn size="sm" variant="ghost" onClick={close}>
                            取消
                        </Btn>
                        <span className="flex-1" />
                        <Btn size="sm" variant="danger" onClick={() => void start()}>
                            卸载
                        </Btn>
                    </>
                )}
                {page === "busy" && (
                    <>
                        <Btn size="sm" variant="ghost" disabled>
                            取消
                        </Btn>
                        <span className="flex-1" />
                        <Btn size="sm" variant="danger" disabled>
                            正在卸载…
                        </Btn>
                    </>
                )}
                {page === "fail" && (
                    <>
                        <span className="flex-1" />
                        <Btn size="sm" variant="outline" onClick={back}>
                            返回
                        </Btn>
                        <Btn size="sm" variant="danger" onClick={() => void start()}>
                            重试
                        </Btn>
                    </>
                )}
                {page === "done" && (
                    <>
                        <span className="flex-1" />
                        <Btn size="sm" variant="primary" onClick={close}>
                            完成
                        </Btn>
                    </>
                )}
                {blocked && (
                    <>
                        <span className="flex-1" />
                        <Btn size="sm" variant="primary" onClick={close}>
                            关闭
                        </Btn>
                    </>
                )}
            </footer>
        </div>
    );
}

/**
 * 确认页正文，同时充当卸载中/未完成的正文：h1 与说明跟着状态换词，卡片和进度条就地长出来。
 *
 * 三态共用一屏是有意的：失败不需要一个新页面，它只是"刚才那条进度条停住了、下面多了一张卡"，
 * 进度条停在哪儿本身就是失败现场（0% 失败与 90% 失败要做的完全是两件事）。
 */
function ConfirmBody({
    page,
    running,
    progress,
    error,
}: {
    page: Page;
    running: boolean;
    progress: Progress | null;
    error: string | null;
}) {
    const busy = page !== "confirm";
    const pct = progress?.pct ?? 0;

    return (
        <div className="page-in flex flex-col">
            <h1 className="text-[19px] leading-[26px] font-semibold text-text-1">
                {busy ? "正在卸载" : "卸载 SideShift"}
            </h1>
            <p className="mt-1.5 text-[13px] leading-[20px] text-text-2">
                {busy
                    ? "卸载过程不联网，窗口保持打开即可。"
                    : "移除程序与本机上的应用数据，包括设置、转换记录和界面缓存。"}
            </p>
            {!busy && (
                <p className="mt-1 text-[11px] leading-[16px] text-text-3">
                    数据目录里的<b className="font-medium text-text-2">转换产物不会被删除</b>
                    ，整合包缓存会一并清除。
                </p>
            )}

            {!busy && running && (
                <Card
                    tone="warn"
                    title="SideShift 正在运行"
                    body="继续会先退出它。正在跑的任务会中断，已完成的部分不会丢。"
                />
            )}

            {busy && (
                <div className="mt-[26px]">
                    <div className="flex items-baseline justify-between gap-3">
                        <span
                            aria-live="polite"
                            className="text-[13px] leading-[20px] font-medium text-text-1"
                        >
                            {page === "fail" ? "清理未完成" : "清理程序与数据…"}
                        </span>
                        {page !== "fail" && (
                            <span className="font-mono text-[12px] leading-[20px] text-text-2 tabular-nums">
                                {Math.round(pct)}%
                            </span>
                        )}
                    </div>
                    <div className="mt-2.5 h-1 overflow-hidden rounded-full bg-rail-track">
                        <span
                            className={cn(
                                "block h-full rounded-full transition-[width] duration-300 ease-out",
                                page === "fail" ? "bg-redstone" : "bg-accent"
                            )}
                            style={{ width: `${pct}%` }}
                        />
                    </div>
                </div>
            )}

            {page === "fail" && (
                <Card
                    tone="bad"
                    title="卸载未完成"
                    body={error ?? "有部分文件没能删掉，关掉还开着的 SideShift 窗口再重试。"}
                />
            )}
        </div>
    );
}

/** 已完成：把"什么留在机器上"报出来。缓存本来该跟着走，所以它出现在这里＝没删掉，给个「打开」让人自己清 */
function DoneBody({
    outcome,
    onOpen,
    error,
}: {
    outcome: Outcome;
    onOpen: (path: string) => void;
    error: string | null;
}) {
    const rows = [
        { label: "转换产物", value: outcome.outputDir },
        { label: "缓存未清除", value: outcome.cacheLeftover },
    ]
        .filter((r) => r.value !== null)
        .map((r) => ({ ...r, open: true }));

    return (
        <div className="page-in flex flex-col">
            <h1 className="text-[19px] leading-[26px] font-semibold text-text-1">
                SideShift 已卸载
            </h1>
            <p className="mt-1.5 text-[13px] leading-[20px] text-text-2">
                程序与应用数据已清除。
            </p>

            {rows.length > 0 && (
                <dl className="mt-[22px] flex flex-col gap-2 border-t border-stroke-soft pt-3.5">
                    {rows.map((r) => (
                        <div
                            key={r.label}
                            className="flex items-baseline gap-3"
                        >
                            <dt className="w-[76px] shrink-0 text-[12px] leading-[18px] text-text-3">
                                {r.label}
                            </dt>
                            <dd className="min-w-0 flex-1 truncate font-mono text-[12px] leading-[18px] text-text-1">
                                {r.value}
                            </dd>
                            {r.open && (
                                <Btn
                                    size="sm"
                                    variant="outline"
                                    onClick={() => onOpen(r.value!)}
                                >
                                    打开
                                </Btn>
                            )}
                        </div>
                    ))}
                </dl>
            )}

            {error && (
                <p className="mt-2 text-[12px] leading-[18px] text-redstone">{error}</p>
            )}
        </div>
    );
}

/** 卡：拦截与失败共用一个壳，只有标题的颜色不同（口径同安装壳的错误卡） */
function Card({
    tone,
    title,
    body,
}: {
    tone: "warn" | "bad";
    title: string;
    body: string;
}) {
    return (
        <div className="mt-[22px] rounded-lg border border-stroke bg-bg-panel px-3 py-2.5">
            <h3
                className={cn(
                    "text-[12px] leading-[18px] font-semibold",
                    tone === "warn" ? "text-gold" : "text-redstone"
                )}
            >
                {title}
            </h3>
            <p className="mt-px text-[11px] leading-[16px] text-text-2">{body}</p>
        </div>
    );
}
