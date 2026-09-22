/**
 * 应用骨架：标题栏 + 侧栏 + 页面栈渲染区
 * 页面注册表按导航栈栈顶渲染；新增页面 = 在 navigation.tsx 扩 key + 此处注册组件
 */
import type { ComponentType } from "react";
import { useEffect } from "react";
import { MotionConfig } from "motion/react";
import { Sidebar } from "@/components/layout/Sidebar";
import { TitleBar } from "@/components/layout/TitleBar";
import { HomePage } from "@/features/home/HomePage";
import { TasksPage } from "@/features/tasks/TasksPage";
import { SettingsPage } from "@/features/settings/SettingsPage";
import { ConvertPage } from "@/features/convert/ConvertPage";
import { TaskDetailPage } from "@/features/task/TaskDetailPage";
import { PackStoreProvider } from "@/lib/pack-store";
import {
    NavigationProvider,
    useNavigation,
    type PageKey,
} from "@/lib/navigation";
import "./App.css";

/** 页面 key → 组件 注册表 */
const pages: Record<PageKey, ComponentType> = {
    home: HomePage,
    tasks: TasksPage,
    settings: SettingsPage,
    convert: ConvertPage,
    task: TaskDetailPage,
};

function Shell() {
    const { entry } = useNavigation();
    const Page = pages[entry.key];

    // 浏览器里把文件丢到拖放卡以外区域时，浏览器默认会导航/下载该文件（整页闪跳）；
    // 窗口级 preventDefault 关掉默认行为，真正的解析仍由 Dropzone 的 drop 处理。
    useEffect(() => {
        const swallow = (e: DragEvent) => e.preventDefault();
        window.addEventListener("dragover", swallow);
        window.addEventListener("drop", swallow);
        return () => {
            window.removeEventListener("dragover", swallow);
            window.removeEventListener("drop", swallow);
        };
    }, []);

    return (
        <div
            // overflow-hidden：桌面应用的外壳永远不该有文档级滚动条。滚动只归 `main.page-scroll`；
            // 少了这道裁剪，侧栏底部气泡那类「绝对定位但常驻 DOM」的隐藏件会把文档撑出常驻滚动条
            className="h-screen flex flex-col overflow-hidden bg-background text-foreground"
            onContextMenu={
                import.meta.env.PROD ? (e) => e.preventDefault() : undefined
            }
        >
            {/* 标题栏横跨整窗顶部（设计稿：logo/应用名在最左，窗口控件在最右） */}
            <TitleBar />
            <div className="flex-1 flex min-h-0">
                <Sidebar />
                {/* 页面内容区：设计稿 MainArea padding [24,32]（纵向 24 / 横向 32） */}
                <main className="page-scroll flex-1 overflow-auto px-8 py-6">
                    <Page />
                </main>
            </div>
        </div>
    );
}

function App() {
    return (
        <NavigationProvider>
            {/* reducedMotion:"user"：跟随系统"减少动态效果"设置，全局降级 motion 动画 */}
            <MotionConfig reducedMotion="user">
                <PackStoreProvider>
                    <Shell />
                </PackStoreProvider>
            </MotionConfig>
        </NavigationProvider>
    );
}

export default App;
