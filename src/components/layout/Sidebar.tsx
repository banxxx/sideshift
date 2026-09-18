import { Home, ListChecks, Settings, Moon, Sun } from "lucide-react";
import { useEffect, useState } from "react";
import { cn } from "@/lib/utils";

export type PageKey = "home" | "tasks" | "settings";

interface SidebarProps {
    current: PageKey;
    onNavigate: (page: PageKey) => void;
}

const navItems: { key: PageKey; label: string; icon: typeof Home }[] = [
    { key: "home", label: "首页", icon: Home },
    { key: "tasks", label: "任务列表", icon: ListChecks },
    { key: "settings", label: "设置", icon: Settings },
];

export function Sidebar({ current, onNavigate }: SidebarProps) {
    const [dark, setDark] = useState(() =>
        document.documentElement.classList.contains("dark")
    );

    useEffect(() => {
        document.documentElement.classList.toggle("dark", dark);
        localStorage.setItem("theme", dark ? "dark" : "light");
    }, [dark]);

    return (
        <aside className="w-56 shrink-0 border-r bg-card/50 flex flex-col">
            {/* 导航 */}
            <nav className="flex-1 p-3 space-y-1">
                {navItems.map((item) => {
                    const Icon = item.icon;
                    const active = current === item.key;
                    return (
                        <button
                            key={item.key}
                            onClick={() => onNavigate(item.key)}
                            className={cn(
                                "w-full flex items-center gap-3 px-3 py-2 rounded-md text-sm transition-colors",
                                active
                                    ? "bg-primary text-primary-foreground"
                                    : "text-muted-foreground hover:bg-accent hover:text-accent-foreground"
                            )}
                        >
                            <Icon className="h-4 w-4" />
                            <span>{item.label}</span>
                        </button>
                    );
                })}
            </nav>

            {/* 底部：版本号 + 主题切换 */}
            <div className="p-3 border-t flex items-center justify-between">
                <span className="text-xs text-muted-foreground">v0.1.0</span>
                <button
                    onClick={() => setDark((v) => !v)}
                    className="p-1.5 rounded-md hover:bg-accent text-muted-foreground hover:text-foreground transition-colors"
                    title={dark ? "切换到亮色" : "切换到暗色"}
                >
                    {dark ? (
                        <Sun className="h-3.5 w-3.5" />
                    ) : (
                        <Moon className="h-3.5 w-3.5" />
                    )}
                </button>
            </div>
        </aside>
    );
}