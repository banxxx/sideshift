/**
 * 卸载壳：一屏五态（确认 / 卸载中 / 未完成 / 已完成 / 找不到安装位置），没有页脚。
 *
 * 两件刻意的事：
 * - 卸载中没有「取消」：这一段是官方 NSIS 卸载器在删文件，壳这边没有能把它停住的口子，
 *   给一颗按下去什么都不发生的钮比不给更糟。安装壳那边有，所以那边给了。
 * - 不替用户杀应用：在跑的 SideShift 由 NSIS 自己按 exe 名结束，壳只报事实并轮询，
 *   人自己退出应用那行提示当场消失。
 *
 * 「同时删除转换产物」默认不勾，且只有产物目录确实是按规则建出来的那一个时才出现（判据在 Rust 侧）。
 * 根节点 overflow-hidden：Tip 只靠 opacity 藏身，气泡一直在布局里，文档级滚动条会把标题栏一起拉动。
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";
import {
    WizardCheck,
    WizardCta,
    WizardHero,
    WizardLink,
    WizardPathLine,
    WizardProgress,
    WizardText,
    WizardTitle,
    WizardTitleBar,
} from "@/components/ui/Wizard";
import {
    checkRunning,
    getSnapshot,
    onProgress,
    runUninstall,
    type Outcome,
    type Progress,
    type Snapshot,
} from "./api";

type Screen = "idle" | "busy" | "failed" | "done" | "blocked";

/** 应用运行中轮询的节拍：够快到人关掉窗口后那行提示就消失，又不至于让 WebView 一直忙 */
const POLL_MS = 900;

export function UninstallerApp() {
    const t = useT();

    const [snap, setSnap] = useState<Snapshot | null>(null);
    const [snapErr, setSnapErr] = useState<string | null>(null);
    const [running, setRunning] = useState(false);
    const [alsoOutput, setAlsoOutput] = useState(false);

    const [screen, setScreen] = useState<Screen>("idle");
    const [progress, setProgress] = useState<Progress | null>(null);
    const [error, setError] = useState<string | null>(null);
    const [outcome, setOutcome] = useState<Outcome | null>(null);

    const alive = useRef(true);
    useEffect(() => {
        alive.current = true;
        getSnapshot()
            .then((s) => {
                if (!alive.current) return;
                setSnap(s);
                setRunning(s.running);
                if (!s.valid) setScreen("blocked");
            })
            .catch((e) => {
                if (!alive.current) return;
                setSnapErr(String(e));
                setScreen("blocked");
            });
        return () => {
            alive.current = false;
        };
    }, []);

    // 只在确认页轮询：进了卸载页，应用是不是还在跑已经不是这个界面要回答的问题
    useEffect(() => {
        if (screen !== "idle" || snap === null) return;
        const id = setInterval(() => {
            checkRunning()
                .then((v) => alive.current && setRunning(v))
                .catch(() => {});
        }, POLL_MS);
        return () => clearInterval(id);
    }, [screen, snap]);

    useEffect(() => {
        const unlisten = onProgress((p) => alive.current && setProgress(p));
        return () => void unlisten.then((f) => f());
    }, []);

    const start = useCallback(async () => {
        setProgress(null);
        setError(null);
        setOutcome(null);
        setScreen("busy");
        // 勾了但目录不是我们造的那一个：以 Rust 的判据为准，这边一律当没勾
        const take = alsoOutput && (snap?.outputMine ?? false);
        try {
            setOutcome(await runUninstall(take));
            setScreen("done");
        } catch (e) {
            setError(String(e));
            setScreen("failed");
        }
    }, [alsoOutput, snap]);

    const back = useCallback(() => {
        setError(null);
        setProgress(null);
        setScreen("idle");
    }, []);

    const close = () => void getCurrentWindow().close();

    // 稿子按 14 的间距排确认页（那一屏东西多），其余四屏按 20
    const gap = screen === "idle" ? "gap-[14px]" : "gap-5";

    return (
        <div className="app-field flex h-screen flex-col overflow-hidden text-text-1">
            <WizardTitleBar
                meta={snap ? `${snap.version} · ${snap.arch}` : undefined}
            />

            <main className="page-scroll flex min-h-0 flex-1 flex-col px-[60px] pb-6">
                <div
                    key={screen}
                    className={cn(
                        "page-in flex w-full flex-1 flex-col items-center justify-center",
                        gap
                    )}
                >
                    <WizardHero />

                    {screen === "idle" && (
                        <>
                            <WizardTitle>
                                {t("wizard.uninstall-title", "卸载 SideShift")}
                            </WizardTitle>
                            <WizardText>
                                {t(
                                    "wizard.uninstall-what",
                                    "删掉程序、设置与整合包缓存。"
                                )}
                            </WizardText>
                            {running && (
                                <WizardText size={11} className="text-gold">
                                    {t(
                                        "wizard.uninstall-running",
                                        "SideShift 正在运行，继续会先退出它。"
                                    )}
                                </WizardText>
                            )}
                            {snap?.outputDir && (
                                <WizardPathLine
                                    before={
                                        alsoOutput && snap.outputMine
                                            ? t("wizard.out-also-a", "转换产物 ")
                                            : t("wizard.out-keep-a", "转换产物留在 ")
                                    }
                                    path={snap.outputDir}
                                    after={
                                        alsoOutput && snap.outputMine
                                            ? t("wizard.out-also-b", " 会一起删掉。")
                                            : t("wizard.out-keep-b", "，不会被删。")
                                    }
                                    danger={alsoOutput && snap.outputMine}
                                />
                            )}
                            {snap?.outputMine && (
                                <WizardCheck
                                    checked={alsoOutput}
                                    onChange={setAlsoOutput}
                                    label={t(
                                        "wizard.also-output",
                                        "同时删除转换产物"
                                    )}
                                />
                            )}
                            <div className="flex flex-col items-center gap-2.5">
                                <WizardCta
                                    kind="danger"
                                    label={t("wizard.uninstall", "卸载")}
                                    onClick={() => void start()}
                                />
                                <WizardLink
                                    tone="text-3"
                                    label={t("wizard.cancel", "取消")}
                                    onClick={close}
                                />
                            </div>
                        </>
                    )}

                    {screen === "busy" && (
                        <>
                            <WizardTitle>
                                {t("wizard.uninstalling", "正在卸载")}
                            </WizardTitle>
                            <WizardProgress
                                pct={progress?.pct ?? 0}
                                label={t(
                                    "wizard.uninstall-stage",
                                    "清理程序与数据"
                                )}
                            />
                        </>
                    )}

                    {screen === "failed" && (
                        <>
                            <WizardTitle>
                                {t("wizard.uninstall-failed", "卸载未完成")}
                            </WizardTitle>
                            <WizardText size={12} className="text-redstone">
                                {error ??
                                    t(
                                        "wizard.uninstall-failed-why",
                                        "有部分文件没能删掉，关掉还开着的 SideShift 窗口再重试。"
                                    )}
                            </WizardText>
                            <div className="flex flex-col items-center gap-2.5">
                                <WizardCta
                                    kind="danger"
                                    label={t("wizard.retry", "重试")}
                                    onClick={() => void start()}
                                />
                                <WizardLink
                                    label={t("wizard.back", "返回")}
                                    onClick={back}
                                />
                            </div>
                        </>
                    )}

                    {screen === "done" && outcome && (
                        <>
                            <WizardTitle>
                                {t("wizard.uninstalled", "SideShift 已卸载")}
                            </WizardTitle>
                            {/* 留在机器上的东西按实测报，不引用开屏那份快照：卸载途中盘被拔掉、
                                目录被人手删，这里得跟着变 */}
                            {outcome.outputDir &&
                                (outcome.outputTargeted ? (
                                    <WizardPathLine
                                        danger
                                        before={t(
                                            "wizard.left-output",
                                            "转换产物没能删掉，还留在 "
                                        )}
                                        path={outcome.outputDir}
                                    />
                                ) : (
                                    <WizardPathLine
                                        before={t(
                                            "wizard.out-keep-a",
                                            "转换产物留在 "
                                        )}
                                        path={outcome.outputDir}
                                        after={t(
                                            "wizard.out-keep-b",
                                            "，不会被删。"
                                        )}
                                    />
                                ))}
                            {outcome.cacheLeftover && (
                                <WizardPathLine
                                    danger
                                    before={t(
                                        "wizard.left-cache",
                                        "整合包缓存没能清干净，还留在 "
                                    )}
                                    path={outcome.cacheLeftover}
                                />
                            )}
                            <WizardCta
                                label={t("wizard.close", "关闭")}
                                onClick={close}
                            />
                        </>
                    )}

                    {screen === "blocked" && (
                        <>
                            <WizardTitle>
                                {t("wizard.no-location", "找不到安装位置")}
                            </WizardTitle>
                            <WizardText size={12}>
                                {snapErr ??
                                    t(
                                        "wizard.no-location-why",
                                        "这个卸载程序必须和 SideShift.exe 装在同一个目录里。"
                                    )}
                            </WizardText>
                            <WizardCta
                                label={t("wizard.close", "关闭")}
                                onClick={close}
                            />
                        </>
                    )}
                </div>
            </main>
        </div>
    );
}
