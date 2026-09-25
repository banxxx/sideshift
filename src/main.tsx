import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { dismissSplash } from "./lib/splash";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);

// 两帧 = 提交完成且浏览器画完，才收启动页（启动页本身还有最短展示时长兜底）
requestAnimationFrame(() => requestAnimationFrame(dismissSplash));
