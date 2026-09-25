/**
 * 应用骨架：标题栏 + 侧栏 + 页面栈渲染区
 * 页面注册表按导航栈栈顶渲染；新增页面 = 在 navigation.tsx 扩 key + 此处注册组件
 */
import type { ComponentType } from "react";
import { useEffect } from "react";
import { AnimatePresence, MotionConfig, motion } from "motion/react";
import { Sidebar } from "@/components/layout/Sidebar";
import { TitleBar } from "@/components/layout/TitleBar";
import { PAGE_IN, PAGE_OUT } from "@/lib/page-motion";
import { HomePage } from "@/features/home/HomePage";
import { TasksPage } from "@/features/tasks/TasksPage";
import { SettingsPage } from "@/features/settings/SettingsPage";
import { ConvertPage } from "@/features/convert/ConvertPage";
import { TaskDetailPage } from "@/features/task/TaskDetailPage";
import { TrashBin } from "@/features/tasks/TrashBin";
import { ResizeEdges } from "@/components/shared/ResizeEdges";
import { useWindowControls } from "@/lib/window-controls";
import { PackStoreProvider } from "@/lib/pack-store";
import {
    EntryFreezer,
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
    // 最大化时把留白收成 0（见 App.css 的「最大化」段）：卡片铺满整窗、投影与把手一起退场
    const { isMaximized } = useWindowControls();

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
        // 两层外壳：外层只负责窗口四周的透明留白（`--win-inset`，投影要向外扩散就得有地方落），
        // 内层才是应用卡片本体。配套口径见 App.css 的「窗口外壳」段与 tauri.conf 的 transparent。
        <div
            data-win-max={isMaximized || undefined}
            className="relative h-screen p-[var(--win-inset)]"
            onContextMenu={
                import.meta.env.PROD ? (e) => e.preventDefault() : undefined
            }
        >
            {/* 卡片必须 overflow-hidden：外壳永远不该有文档级滚动条，滚动只归 `main.page-scroll`
                （否则侧栏底部气泡那类「绝对定位但常驻 DOM」的隐藏件会把文档撑出常驻滚动条）。 */}
            <div className="h-full flex flex-col overflow-hidden bg-background text-foreground shadow-[var(--win-shadow)]">
                {/* 标题栏横跨整窗顶部（设计稿：logo/应用名在最左，窗口控件在最右） */}
                <TitleBar />
                <div className="flex-1 flex min-h-0">
                    <Sidebar />
                    {/* 页面内容区：设计稿 MainArea padding [24,32]（纵向 24 / 横向 32）。
                        纵向留白**由各页自己的根元素带**（见下），这里只留横向：
                        滚动容器自带的 padding-top 是吸顶页头盖不住的一条带子——sticky 的贴合边落在
                        内容盒（padding 之下），页头永远差着这 24px，卡片从缝里露出来。
                        契约：每个页面根元素 = `py-6`（任务列表页把 pt-6 放进吸顶盒里，好让贴合边=留白起点）。
                        换页串行（mode="wait"：旧页退场演完才挂新页），所以屏上永远只有一层页面——
                        滚动容器仍归 main，不必下放、吸顶页头也不会两层叠印。
                        key 只取页名：同一页内的数据刷新（筛选变化、轮询到新任务）不该重播整页入场。
                        EntryFreezer 把栈顶钉在挂载那一刻：退场那一拍栈已经换了，旧页若跟着读新栈顶，
                        任务详情页会读不到 taskId 而先闪一句「任务不存在或已过期」。见 navigation.tsx。 */}
                    <AnimatePresence initial={false} mode="wait">
                        <motion.main
                            key={entry.key}
                            initial={{ opacity: 0, y: 24 }}
                            animate={{ opacity: 1, y: 0, transition: PAGE_IN }}
                            exit={{ opacity: 0, y: -10, transition: PAGE_OUT }}
                            className="page-scroll flex-1 overflow-auto px-8"
                        >
                            <EntryFreezer entry={entry}>
                                <Page />
                            </EntryFreezer>
                        </motion.main>
                    </AnimatePresence>
                    {/* 回收站入口：只在任务列表页挂着——删除与飞行落点都发生在那一页，别的页面
                        亮一个够不着的垃圾桶只是噪音。仍在 Shell 层而不是塞进 TasksPage：
                        它是 `fixed` 的落点，留在滚动容器外面不必去赌「fixed 后代是否被滚动容器裁」这一类
                        实现细节，而且它一直挂在 DOM 里（空回收站时透明），飞行才量得到中心。 */}
                    {entry.key === "tasks" && <TrashBin />}
                </div>
            </div>
            {/* 缩放把手铺在四周留白里，必须排在卡片之后（同 z 档时后来者压过模态遮罩）。
                口径见组件文件头：透明留白把系统那条原生缩放环推到了窗口外沿，这里补回边框。 */}
            <ResizeEdges />
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
