/**
 * 安装壳：一屏四态（待装 / 安装中 / 未完成 / 装完），没有页脚也没有流程条。
 *
 * 「自定义安装路径」是就地展开一行路径框（`expanded` 一轴），不是第二页：展开态与收起态同属"还没开始装"，
 * 取消、失败回来时应该回到原样。数据跟着安装目录走（`appdata`/`cache`/`output` 落在安装目录里），
 * 所以界面只问一个目录，不再单独问"数据放哪"。
 *
 * 根节点 overflow-hidden：Tip 只靠 opacity 藏身，气泡一直在布局里，文档级滚动条会把标题栏一起拉动。
 */
import { useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open } from "@tauri-apps/plugin-dialog";
import { useT, type TranslateFn } from "@/lib/i18n";
import { cn } from "@/lib/utils";
import {
    WizardCta,
    WizardHero,
    WizardLink,
    WizardPathLine,
    WizardPathRow,
    WizardProgress,
    WizardText,
    WizardTitle,
    WizardTitleBar,
} from "@/components/ui/Wizard";
import {
    cancelInstall,
    getPlan,
    launchApp,
    onProgress,
    runInstall,
    type Outcome,
    type Plan,
    type Progress,
    type Stage,
} from "./api";

type Screen = "idle" | "busy" | "failed" | "done";

/** 阶段短名：三句都写成字面量调用点（`t(键, 原文)` 的形状扫描器才认），求值留给渲染时——
 *  模块顶层求值会把词冻在首次加载的语言上 */
function stageLabel(stage: Stage, t: TranslateFn) {
    switch (stage) {
        case "prepare":
            return t("wizard.stage-prepare", "准备安装包");
        case "copy":
            return t("wizard.stage-copy", "复制程序文件");
        case "data":
            return t("wizard.stage-data", "建立数据目录");
    }
}

export function InstallerApp() {
    const t = useT();

    const [plan, setPlan] = useState<Plan | null>(null);
    const [dir, setDir] = useState("");
    const [expanded, setExpanded] = useState(false);

    const [screen, setScreen] = useState<Screen>("idle");
    const [progress, setProgress] = useState<Progress | null>(null);
    const [error, setError] = useState<string | null>(null);
    const [outcome, setOutcome] = useState<Outcome | null>(null);
    const [launchErr, setLaunchErr] = useState<string | null>(null);

    /** 用户主动取消过：run_install 的 reject 不该当失败处理 */
    const canceling = useRef(false);

    useEffect(() => {
        let alive = true;
        getPlan()
            .then((p) => {
                if (!alive) return;
                setPlan(p);
                setDir(p.installDir);
            })
            .catch((e) => alive && setError(String(e)));
        return () => {
            alive = false;
        };
    }, []);

    useEffect(() => {
        let alive = true;
        const unlisten = onProgress((p) => alive && setProgress(p));
        return () => {
            alive = false;
            void unlisten.then((f) => f());
        };
    }, []);

    const browse = async () => {
        const chosen = await open({
            directory: true,
            defaultPath: dir,
            title: t("wizard.pick-dir", "选择安装位置"),
        });
        if (typeof chosen === "string") setDir(chosen);
    };

    const start = async () => {
        if (!dir) return;
        canceling.current = false;
        setProgress(null);
        setError(null);
        setOutcome(null);
        setScreen("busy");
        try {
            setOutcome(await runInstall(dir));
            setScreen("done");
        } catch (e) {
            if (canceling.current) {
                // 取消不是失败：回到刚才那一屏让人改主意，别挂红字
                setScreen("idle");
                return;
            }
            setError(String(e));
            setScreen("failed");
        }
    };

    const cancel = () => {
        canceling.current = true;
        void cancelInstall();
    };

    /** 装完顺手把应用拉起来：启动失败就别关窗，装完却没打开等于把人退路也收了 */
    const finish = async () => {
        if (!outcome) return;
        try {
            await launchApp(outcome.installedExe);
        } catch (e) {
            setLaunchErr(String(e));
            return;
        }
        void getCurrentWindow().close();
    };

    const stage = progress ? stageLabel(progress.stage, t) : "";
    // 稿子按 22 的间距排待装那一屏（它要有呼吸），其余三屏按 20
    const gap = screen === "idle" ? "gap-[22px]" : "gap-5";

    return (
        <div className="app-field flex h-screen flex-col overflow-hidden text-text-1">
            <WizardTitleBar
                meta={plan ? `${plan.version} · ${plan.arch}` : undefined}
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
                            <div className="flex flex-col items-center gap-1.5">
                                <WizardTitle>SideShift</WizardTitle>
                                <WizardText className="text-text-3">
                                    {t(
                                        "wizard.tag",
                                        "Minecraft 整合包转换工具"
                                    )}
                                </WizardText>
                            </div>
                            <div className="flex flex-col items-center gap-2.5">
                                <WizardLink
                                    label={t(
                                        "wizard.custom-path",
                                        "自定义安装路径"
                                    )}
                                    chevron={expanded ? "up" : "down"}
                                    tone={expanded ? "text-1" : "text-2"}
                                    onClick={() => setExpanded((v) => !v)}
                                />
                                {expanded && (
                                    <WizardPathRow
                                        value={dir}
                                        button={t("wizard.change", "更改")}
                                        ariaLabel={t(
                                            "wizard.pick-dir",
                                            "选择安装位置"
                                        )}
                                        onBrowse={() => void browse()}
                                    />
                                )}
                                <WizardCta
                                    label={t("wizard.install", "安装")}
                                    disabled={!dir}
                                    onClick={() => void start()}
                                />
                            </div>
                        </>
                    )}

                    {screen === "busy" && (
                        <>
                            <WizardTitle>
                                {t("wizard.installing", "正在安装")}
                            </WizardTitle>
                            <WizardProgress
                                pct={progress?.pct ?? 0}
                                label={stage}
                            />
                            <WizardLink
                                label={t("wizard.cancel-install", "取消安装")}
                                tone="text-3"
                                onClick={cancel}
                            />
                        </>
                    )}

                    {screen === "failed" && (
                        <>
                            <WizardTitle>
                                {t("wizard.install-failed", "安装未完成")}
                            </WizardTitle>
                            <WizardText size={12} className="text-redstone">
                                {error}
                            </WizardText>
                            <div className="flex flex-col items-center gap-2.5">
                                <WizardCta
                                    label={t("wizard.retry", "重试")}
                                    onClick={() => void start()}
                                />
                                <WizardLink
                                    label={t("wizard.pick-else", "换个位置")}
                                    onClick={() => {
                                        setExpanded(true);
                                        setError(null);
                                        setScreen("idle");
                                    }}
                                />
                            </div>
                        </>
                    )}

                    {screen === "done" && outcome && (
                        <>
                            <div className="flex flex-col items-center gap-1.5">
                                <WizardTitle>
                                    {t("wizard.installed", "安装完成")}
                                </WizardTitle>
                                <WizardPathLine
                                    before={t("wizard.installed-to", "已装到 ")}
                                    path={outcome.installDir}
                                />
                            </div>
                            <WizardCta
                                label={t("wizard.open", "打开 SideShift")}
                                onClick={() => void finish()}
                            />
                            {/* 稿子没画的两个现场：卸载入口没换成这套界面、以及点了打开却没起来。
                                都不是"装失败"，所以留在完成屏，只把话说明白 */}
                            {outcome.uninstallNote && (
                                <WizardText size={11} className="text-gold">
                                    {outcome.uninstallNote}
                                </WizardText>
                            )}
                            {launchErr && (
                                <WizardText size={11} className="text-redstone">
                                    {launchErr}
                                </WizardText>
                            )}
                        </>
                    )}
                </div>
            </main>
        </div>
    );
}
