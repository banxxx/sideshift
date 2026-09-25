/**
 * 左侧导航栏（SS.pen 各帧 Sidebar：宽 212 / 品牌行 + 预发布徽章（稳定版不渲染）/ 导航行 36 高 / 底部版本+主题切换）
 *
 * 直接消费导航上下文：点一级页 = 清栈切换；二级页期间其所属一级页保持高亮。
 * 高亮样式按设计稿：accent-dim 底 + accent 图标文字（不是实心 accent 底）。
 * 淡底不是每行各画一块，而是一颗共享胶囊（layoutId="nav-pill"）在行间滑：
 * 跨行也滑（设置→首页从任务列表那行穿过去），与分段页签的胶囊同一招、同一条弹簧。
 *
 * 底部那行守「信息在左、动作在右」的全应用排布语法：版本号靠左（24px，正好对上导航行
 * 图标的左缘），主题钮靠右（12px，正好对上导航胶囊的右缘，也与标题栏窗口控件同一条竖线）。
 */
import { Home, ListChecks, Settings, Moon, Sun } from "lucide-react";
import { useRef, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import { NotificationStack } from "./NotificationStack";
import { HOVER_FILL } from "@/components/ui";
import { PILL_SLIDE } from "@/lib/springs";
import { cn } from "@/lib/utils";
import { isDark, switchTheme, useTheme } from "@/lib/theme";
import { PRERELEASE_BADGE, VERSION_CORE } from "@/lib/api";
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
    const [theme] = useTheme();
    const dark = isDark();
    const themeBtn = useRef<HTMLButtonElement>(null);
    // 每点一次换一个 key 重播光环；不需要退场，终态本身就是透明
    const [pulse, setPulse] = useState(0);

    /** 快切只两态：点下去即锁定浅/深，因此读屏文案先把「这会离开跟随系统」说破（按钮无图标外的可见文字，靠它当名字） */
    const next = dark ? "light" : "dark";
    const themeLabel =
        theme === "system"
            ? `跟随系统（当前${dark ? "深色" : "亮色"}）· 点击锁定为${next === "light" ? "亮色" : "深色"}`
            : `切换到${next === "light" ? "亮色" : "深色"}`;

    /** 当前一级页：栈顶是一级则取自身，二级则回溯到所属一级 */
    const activePrimary: PrimaryPage =
        entry.key in SECONDARY_OWNER
            ? SECONDARY_OWNER[entry.key as keyof typeof SECONDARY_OWNER]
            : (entry.key as PrimaryPage);

    return (
        <aside className="w-[212px] shrink-0 bg-bg-panel border-r border-stroke-soft flex flex-col px-3 pt-4 pb-0">
            {/* 品牌行：SideShift + 预发布徽章 */}
            <div className="h-7 flex items-center gap-1.5 px-2">
                <span className="font-heading text-base font-bold tracking-tight text-text-1">
                    SideShift
                </span>
                {/* 预发布徽章：文案由版本号推导（见 api 的 PRERELEASE_BADGE），稳定版整枚不渲染。
                    行高必须写死：html 的 24px 行高会继承进来，不锁就把 9px 字撑成 28px 高的胶囊。
                    圆角不走 sm(6)——盒子只有 16 高，6 已经读作胶囊了。 */}
                {PRERELEASE_BADGE && (
                    <span className="inline-flex h-4 shrink-0 items-center rounded-[4px] bg-accent-dim px-1.5 text-[9px] font-semibold leading-none tracking-[0.04em] text-accent">
                        {PRERELEASE_BADGE}
                    </span>
                )}
            </div>

            <div className="h-5" />

            {/* 导航：行 36 高 r8，激活 accent 字；淡底交给一颗共享胶囊（layoutId），
                切分类时它从旧行滑到新行——跨行也滑，不瞬移换色 */}
            <nav className="flex flex-col gap-1">
                {navItems.map((item) => {
                    const Icon = item.icon;
                    const active = activePrimary === item.key;
                    return (
                        <button
                            key={item.key}
                            onClick={() => switchPrimary(item.key)}
                            className={cn(
                                "relative w-full h-9 rounded-lg px-3 text-[13px] font-medium",
                                HOVER_FILL,
                                active
                                    ? "text-accent"
                                    : "text-text-2 hover:bg-surface-2 hover:text-text-1"
                            )}
                        >
                            {active && (
                                <motion.span
                                    layoutId="nav-pill"
                                    transition={PILL_SLIDE}
                                    className="absolute inset-0 rounded-lg bg-accent-dim"
                                />
                            )}
                            <span className="relative z-[1] flex h-full items-center gap-2.5">
                                <Icon className="size-4" />
                                <span>{item.label}</span>
                            </span>
                        </button>
                    );
                })}
            </nav>

            <div className="flex-1" />

            {/* 底部：版本号 + 主题切换（32×32 surface-2 r8）。
                `relative` 是给提示区当定位参照的：提示区挂在这一行的上方边缘（`bottom-full`），
                彻底不进侧栏的文档流——它长多高、什么时候塌掉都不该动到别的控件。
                顺带把 popLayout 退场件的位置参照也收在这圈里（原先侧栏没有 positioned 祖先，
                被抽成 absolute 的退场卡片是挂到整窗外壳那个 `relative` 上的）。 */}
            <div className="relative flex items-center justify-between pt-3 pr-0 pb-3 pl-3">
                <NotificationStack />
                <span className="font-mono text-[11px] text-text-3">v{VERSION_CORE}</span>
                <button
                    ref={themeBtn}
                    onClick={() => {
                        setPulse((n) => n + 1);
                        // 圆心取这枚按钮：颜色从手指落点铺开才读得出因果（见 switchTheme 的时序说明）
                        switchTheme(next, themeBtn.current);
                    }}
                    aria-label={themeLabel}
                    className={cn(
                        "relative size-8 rounded-lg bg-surface-2 flex items-center justify-center text-text-2 hover:text-text-1",
                        HOVER_FILL
                    )}
                >
                    {pulse > 0 && (
                        <motion.span
                            key={pulse}
                            aria-hidden
                            initial={{ opacity: 0.5, scale: 1 }}
                            animate={{ opacity: 0, scale: 1.18 }}
                            transition={{ duration: 0.13, ease: "easeOut" }}
                            className="pointer-events-none absolute inset-0 rounded-lg border border-accent"
                        />
                    )}
                    {/* 换图与脉冲同时起步、赶在展开前落定（各自时长 ≈ PRE_DELAY）：
                        View Transition 一起，真 DOM 就被快照定格，没演完的部分会僵在那一帧 */}
                    <span className="relative flex size-3.5 items-center justify-center">
                        <AnimatePresence initial={false}>
                            <motion.span
                                key={dark ? "dark" : "light"}
                                initial={{ opacity: 0, rotate: -90, scale: 0.6 }}
                                animate={{ opacity: 1, rotate: 0, scale: 1 }}
                                exit={{ opacity: 0, rotate: 90, scale: 0.6 }}
                                transition={{ duration: 0.14, ease: [0.16, 1, 0.3, 1] }}
                                className="absolute inset-0 flex items-center justify-center"
                            >
                                {dark ? <Sun className="size-3.5" /> : <Moon className="size-3.5" />}
                            </motion.span>
                        </AnimatePresence>
                    </span>
                </button>
            </div>
        </aside>
    );
}
