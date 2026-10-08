import React from "react";
import ReactDOM from "react-dom/client";
import "./installer.css";
// 只取这套主题存储态：没设置过就是 system（跟随深浅偏好），壳里没有开关去覆盖它。
// 不引这条链的话 html 上的 .dark 类再没人维护，系统深色下会渲染成浅色面板
import "@/lib/theme";
import { initI18n, t } from "@/lib/i18n";
import { InstallerApp } from "./InstallerApp";

/**
 * 语言与主题同一个口径：壳的 WebView 档案是独立一份（`data_directory` 不落应用那套），
 * localStorage 那面镜像在这儿恒空 ⇒ `initI18n()` 直接落回系统语言。挂树前 await 它，
 * 第一帧就是最终语言，不会先闪一次中文再换过去（口径同主应用 main.tsx）。
 */
void initI18n().then(() => {
    document.title = t("wizard.win-title", "SideShift 安装");
    ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
        <React.StrictMode>
            <InstallerApp />
        </React.StrictMode>
    );
});
