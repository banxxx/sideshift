/**
 * 安装壳的主组件：三页一步的直线流程 + 页脚动作
 *
 * 状态机刻意只有 page 一轴（0 存放位置 / 1 安装中 / 2 完成），失败不另立一页：
 * 失败就是"安装中这一页出了错"，就地重试或回第 0 页改目录。
 *
 * 「上一步」常驻、只在安装失败时放开：跑起来之后没有能回头改的东西，但按钮一藏，
 * 三页的位置感就没了（人以为流程只有两步）。所以留灰不给点。
 *
 * 根节点 overflow-hidden：Tip 只靠 opacity 藏身，气泡一直在布局里，
 * 文档级滚动条会把标题栏一起拉动（口径同主应用 App 根）。
 */
import { useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Btn } from "@/components/ui/Button";
import { cn } from "@/lib/utils";
import {
    cancelInstall,
    getPlan,
    launchApp,
    onProgress,
    resolveLayout,
    runInstall,
    type Layout,
    type Outcome,
    type Plan,
    type Progress,
} from "./api";
import { DonePage } from "./DonePage";
import { InstallPage } from "./InstallPage";
import { LocationPage } from "./LocationPage";
import { TitleBar } from "./TitleBar";

/** 每步在流程条上的短名 */
const STEP_LABELS = ["选择安装位置", "安装", "安装完成"] as const;

type Page = 0 | 1 | 2;

export function InstallerApp() {
    const [plan, setPlan] = useState<Plan | null>(null);
    const [sel, setSel] = useState<Layout | null>(null);
    const [installDir, setInstallDir] = useState("");
    const [pickErr, setPickErr] = useState<string | null>(null);

    const [page, setPage] = useState<Page>(0);
    const [progress, setProgress] = useState<Progress | null>(null);
    const [installErr, setInstallErr] = useState<string | null>(null);
    const [outcome, setOutcome] = useState<Outcome | null>(null);
    const [launch, setLaunch] = useState(true);
    const [launchErr, setLaunchErr] = useState<string | null>(null);

    /** 用户主动取消过：run_install 的 reject 就不该当失败处理 */
    const canceling = useRef(false);

    const pickRoot = async (root: string) => {
        try {
            setPickErr(null);
            setSel(await resolveLayout(root));
        } catch (e) {
            setPickErr(String(e));
        }
    };

    useEffect(() => {
        let alive = true;
        getPlan()
            .then(async (p) => {
                if (!alive) return;
                setPlan(p);
                setInstallDir(p.installDir);
                // 预选档与回落根都过一遍 resolve_layout：布局规则只有 Rust 那一份
                await pickRoot(p.drives[0]?.dataRoot ?? p.fallbackDataRoot);
            })
            .catch((e) => alive && setPickErr(String(e)));
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

    const start = async () => {
        if (!sel) return;
        canceling.current = false;
        setProgress(null);
        setInstallErr(null);
        setOutcome(null);
        setPage(1);
        try {
            setOutcome(await runInstall(sel.dataRoot, installDir));
            setPage(2);
        } catch (e) {
            if (canceling.current) {
                // 取消不是失败：回第 0 页让人改主意，别挂红卡
                setPage(0);
                return;
            }
            setInstallErr(String(e));
        }
    };

    /** 回到第 0 页改目录：失败提示跟着清掉，否则回去再进来还挂着上次的错 */
    const back = () => {
        setInstallErr(null);
        setProgress(null);
        setPage(0);
    };

    const close = () => void getCurrentWindow().close();

    const finish = async () => {
        if (launch && outcome) {
            try {
                await launchApp(outcome.installedExe);
            } catch (e) {
                // 启动失败就别关窗：装完却没打开，人连退路都没了
                setLaunchErr(String(e));
                return;
            }
        }
        close();
    };

    const running = page === 1 && !installErr;

    return (
        <div className="flex h-screen flex-col overflow-hidden bg-bg-app text-text-1">
            <TitleBar />
            <FlowBar page={page} plan={plan} />

            <main className="page-scroll min-h-0 flex-1 overflow-y-auto px-7 pt-4 pb-5">
                {page === 0 && (
                    <LocationPage
                        plan={plan}
                        sel={sel}
                        installDir={installDir}
                        error={pickErr}
                        onPickRoot={(root) => void pickRoot(root)}
                        onPickInstallDir={setInstallDir}
                    />
                )}
                {page === 1 && (
                    <InstallPage
                        progress={progress}
                        error={installErr}
                    />
                )}
                {page === 2 && outcome && (
                    <DonePage
                        outcome={outcome}
                        launch={launch}
                        onToggleLaunch={setLaunch}
                        error={launchErr}
                    />
                )}
            </main>

            <footer className="flex shrink-0 items-center gap-2 border-t border-stroke bg-bg-panel px-7 py-3">
                <Btn
                    size="sm"
                    variant="ghost"
                    onClick={() => {
                        if (running) {
                            canceling.current = true;
                            void cancelInstall();
                        } else close();
                    }}
                >
                    {running ? "取消安装" : "取消"}
                </Btn>
                <span className="flex-1" />
                <Btn
                    size="sm"
                    variant="outline"
                    disabled={!(page === 1 && !!installErr)}
                    onClick={back}
                    className="disabled:pointer-events-auto disabled:cursor-not-allowed disabled:opacity-45"
                >
                    上一步
                </Btn>
                {page === 0 && (
                    <Btn size="sm" variant="primary" disabled={!sel} onClick={() => void start()}>
                        安装
                    </Btn>
                )}
                {running && (
                    <Btn size="sm" variant="primary" disabled>
                        正在安装…
                    </Btn>
                )}
                {page === 1 && installErr && (
                    <Btn size="sm" variant="primary" onClick={() => void start()}>
                        重试
                    </Btn>
                )}
                {page === 2 && (
                    <Btn size="sm" variant="primary" onClick={() => void finish()}>
                        {launch ? "打开 SideShift" : "完成"}
                    </Btn>
                )}
            </footer>
        </div>
    );
}

/**
 * 流程条：文字计数 + 三段 2px 线。
 *
 * 不用胶囊和圆形数字徽章：那套是给"可以随便跳"的多页导航用的，这里只有一条直线，
 * 强调色落在"当前这一段"就够了。右侧的版本 + 架构是装机现场唯一的自查入口。
 */
function FlowBar({ page, plan }: { page: Page; plan: Plan | null }) {
    return (
        <nav className="shrink-0 px-7 pt-3.5">
            <div className="flex items-baseline justify-between gap-4">
                <span className="text-[12px] leading-[18px] font-medium text-text-2">
                    第 <b className="font-semibold text-text-1">{page + 1}</b> 步，共{" "}
                    {STEP_LABELS.length} 步 · {STEP_LABELS[page]}
                </span>
                <span className="font-mono text-[11px] leading-[18px] text-text-3 tabular-nums">
                    {plan ? `${plan.version} · ${plan.arch}` : "正在读取本机磁盘…"}
                </span>
            </div>
            <div className="mt-2.5 flex gap-1">
                {STEP_LABELS.map((label, i) => (
                    <span
                        key={label}
                        className={cn(
                            "h-0.5 flex-1 rounded-full transition-colors duration-200",
                            i <= page ? "bg-accent" : "bg-stroke"
                        )}
                    />
                ))}
            </div>
        </nav>
    );
}
