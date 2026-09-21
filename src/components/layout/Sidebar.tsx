/**
 * 左侧导航栏（SS.pen 各帧 Sidebar：宽 212 / 品牌行 + BETA 徽章 / 导航行 36 高 / 底部版本+主题切换）
 *
 * 直接消费导航上下文：点一级页 = 清栈切换；二级页期间其所属一级页保持高亮。
 * 高亮样式按设计稿：accent-dim 底 + accent 图标文字（不是实心 accent 底）。
 */
import { Home, ListChecks, Settings, Moon, Sun } from "lucide-react";
import { NotificationStack } from "./NotificationStack";
import { Tip, TIP_TRIGGER } from "@/components/ui";
import { cn } from "@/lib/utils";
import { isDark, useTheme } from "@/lib/theme";
import {
    SECONDARY_OWNER,
    useNavigation,
    type PrimaryPage,
} from "@/lib/navigation";

const navItems: { key: PrimaryPage; label: string; icon: typeof Home }[] = [
    { key: "home", label: "首页", icon: Home },
    { key: "tasks", label: "任务列表", icon: ListChecks },
    { key: "settings", label: "设置", icon: Settings },
];

export function Sidebar() {
    const { entry, switchPrimary } = useNavigation();
    const [, setTheme] = useTheme();
    const dark = isDark();

    /** 当前一级页：栈顶是一级则取自身，二级则回溯到所属一级 */
    const activePrimary: PrimaryPage =
        entry.key in SECONDARY_OWNER
            ? SECONDARY_OWNER[entry.key as keyof typeof SECONDARY_OWNER]
            : (entry.key as PrimaryPage);

    return (
        <aside className="w-[212px] shrink-0 bg-bg-panel border-r border-stroke-soft flex flex-col px-3 pt-4 pb-0">
            {/* 品牌行：SideShift + BETA 徽章 */}
            <div className="h-7 flex items-center gap-2.5 px-2">
                <span className="font-heading text-base font-bold tracking-tight text-text-1">
                    SideShift
                </span>
                <span className="rounded-full bg-accent-dim px-[7px] py-0.5 text-[10px] font-semibold text-accent">
                    BETA
                </span>
            </div>

            <div className="h-5" />

            {/* 导航：行 36 高 r8，激活 accent-dim 底 + accent 字 */}
            <nav className="flex flex-col gap-1">
                {navItems.map((item) => {
                    const Icon = item.icon;
                    const active = activePrimary === item.key;
                    return (
                        <button
                            key={item.key}
                            onClick={() => switchPrimary(item.key)}
                            className={cn(
                                "w-full h-9 rounded-lg flex items-center gap-2.5 px-3 text-[13px] font-medium transition-colors",
                                active
                                    ? "bg-accent-dim text-accent"
                                    : "text-text-2 hover:bg-surface-2 hover:text-text-1"
                            )}
                        >
                            <Icon className="size-4" />
                            <span>{item.label}</span>
                        </button>
                    );
                })}
            </nav>

            <div className="flex-1" />

            {/* 提示区：版本号/主题行上方的预留槽位，全局 notify() 统一在此渲染 */}
            <NotificationStack />

            {/* 底部：版本号 + 主题切换（32×32 surface-2 r8） */}
            <div className="flex items-center justify-between pt-3 pr-2 pb-3 pl-3">
                <span className="font-mono text-[11px] text-text-3">v0.1.0</span>
                <button
                    onClick={() => setTheme(dark ? "light" : "dark")}
                    aria-label={dark ? "切换到亮色" : "切换到暗色"}
                    className={cn(
                        TIP_TRIGGER,
                        "size-8 rounded-lg bg-surface-2 flex items-center justify-center text-text-2 hover:text-text-1 transition-colors"
                    )}
                >
                    {dark ? <Sun className="size-3.5" /> : <Moon className="size-3.5" />}
                    {/* side="top"：这枚按钮离窗底只剩 pb-3，气泡朝下长会把整窗撑出一道常驻滚动条 */}
                    <Tip label={dark ? "切换到亮色" : "切换到暗色"} side="top" />
                </button>
            </div>
        </aside>
    );
}
