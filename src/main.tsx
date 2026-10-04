import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { CrashPanel, ErrorBoundary } from "@/components/shared/ErrorBoundary";
import { getSettings } from "@/lib/api/settings";
import { APP_VERSION, updateOutcome } from "@/lib/api";
import { installErrorGuard } from "./lib/error-guard";
import { dismissSplash, splashGone } from "./lib/splash";
import { notify } from "@/lib/notify";
import { initI18n, t } from "@/lib/i18n";

// 兜底要早于任何一次异步取数：挂得晚了，第一条没人 catch 的 rejection 就漏掉了
installErrorGuard();

// 语言在挂树之前定档：目录是打进 bundle 的本地 JSON，等这一步只是等它同步装配完，
// 不等就会有一帧按兜底档画出去（非中文档先闪一次中文）。
// 这里用 async IIFE 而不是顶层 await，是为了不把构建目标抬到 es2022。
// 设置里的档位以回调传入（i18n 不自己 import api，否则会和 `lib/*` 的文案模块成环）
void (async () => {
  const root = ReactDOM.createRoot(document.getElementById("root") as HTMLElement);
  try {
    await initI18n(() => getSettings().then((s) => s.locale));

    root.render(
      <React.StrictMode>
        <ErrorBoundary>
          <App />
        </ErrorBoundary>
      </React.StrictMode>,
    );

    // 两帧 = 提交完成且浏览器画完，才收启动页（启动页本身还有最短展示时长兜底）
    requestAnimationFrame(() => requestAnimationFrame(dismissSplash));

    // 上次「重启并安装」的结论：那一刻我们已经被安装器杀掉了，没有任何当场回执 ⇒ 成败只能这趟启动对账。
    // 挂在挂树之后而不是某个组件的 effect 里：这本账整个进程只该报一次，而切语言会把 LocaleGate 下面
    // 整棵子树重挂，effect 再跑一遍就会把一条早就消失的提示重新推上屏。
    // 等 `splashGone`：卡的停留时间从推入那一拍开始走，遮罩底下走完大半等于没提示过。
    void Promise.all([splashGone, updateOutcome()]).then(([, o]) => {
      if (!o) return;
      notify(
        o.kind === "done"
          ? t("update.outcome-done", "已更新到 v{{version}}", { version: o.attempted })
          : t("update.outcome-unfinished", "更新到 v{{version}} 没有成功，本机仍是 v{{current}}", {
              version: o.attempted,
              current: APP_VERSION,
            }),
        o.kind === "done" ? "success" : "error",
      );
    });
  } catch (e) {
    // 挂树之前就失败：ErrorBoundary 还没在场、notify 也没人读 ⇒ 面板自己画出去，启动页照样收掉
    console.error("[boot]", e);
    root.render(
      <CrashPanel
        detail={e instanceof Error ? `${e.name}: ${e.message}` : String(e)}
      />,
    );
    dismissSplash();
  }
})();
