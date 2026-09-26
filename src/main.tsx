import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { getSettings } from "@/lib/api/settings";
import { dismissSplash } from "./lib/splash";
import { initI18n } from "./lib/i18n";

// 语言在挂树之前定档：目录是打进 bundle 的本地 JSON，等这一步只是等它同步装配完，
// 不等就会有一帧按兜底档画出去（非中文档先闪一次中文）。
// 这里用 async IIFE 而不是顶层 await，是为了不把构建目标抬到 es2022。
// 设置里的档位以回调传入（i18n 不自己 import api，否则会和 `lib/*` 的文案模块成环）
void (async () => {
  await initI18n(() => getSettings().then((s) => s.locale));

  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>,
  );

  // 两帧 = 提交完成且浏览器画完，才收启动页（启动页本身还有最短展示时长兜底）
  requestAnimationFrame(() => requestAnimationFrame(dismissSplash));
})();
