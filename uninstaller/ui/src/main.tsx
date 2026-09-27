import React from "react";
import ReactDOM from "react-dom/client";
import "./uninstaller.css";
// 只取这套主题存储态：没设置过就是 system（跟随深浅偏好），壳里没有开关去覆盖它。
// 不引这条链的话 html 上的 .dark 类再没人维护，系统深色下会渲染成浅色面板
import "@/lib/theme";
import { UninstallerApp } from "./UninstallerApp";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
        <UninstallerApp />
    </React.StrictMode>
);
