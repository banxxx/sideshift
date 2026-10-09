/**
 * 左侧导航栏（宽 212）：品牌行 + 导航行 + 底部版本/主题行；点一级页＝清栈切换，二级页期间其所属一级页保持高亮。
 * 高亮淡底不是每行各画一块，而是一颗共享胶囊（layoutId="nav-pill"）在行间滑，与分段页签同一招、同一条弹簧。
 * 底部守「信息在左、动作在右」的全应用排布语法：版本号靠左（24px 对导航图标左缘），主题钮靠右（12px，与窗口控件同一条竖线）。
 */
import { Moon, Sun } from "lucide-react";
import { useEffect, useRef, useState, type ComponentType, type Ref } from "react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { NotificationStack } from "./NotificationStack";
import { HOVER_FILL, Logo, TIP_TRIGGER, Tip } from "@/components/ui";
import { HomeIcon } from "@/components/icons/home";
import { AlignRightIcon } from "@/components/icons/align-right";
import { LayoutPanelTopIcon } from "@/components/icons/layout-panel-top";
import { BadgeAlertIcon } from "@/components/icons/badge-alert";
import { SettingsIcon } from "@/components/icons/settings";
import { useT } from "@/lib/i18n";
import { PILL_SLIDE } from "@/lib/springs";
import { cn } from "@/lib/utils";
import { isDark, switchTheme, useTheme } from "@/lib/theme";
import { PRERELEASE_BADGE, VERSION_CORE } from "@/lib/api";
import { openUpdate, useUpdateBadge } from "@/lib/update-store";
import {
    SECONDARY_OWNER,
    useNavigation,
    type PrimaryPage,
} from "@/lib/navigation";

/** 动效图标的手柄与组件形状（五个文件各自导出同构的 handle，这里取共同形状） */
type NavIconHandle = { startAnimation: () => void; stopAnimation: () => void };
type NavIcon = ComponentType<{
    size?: number;
    className?: string;
    ref?: Ref<NavIconHandle>;
}>;

/** 导航五行的图标与 id（id 是路由 key，绝不翻译）；页名见下面组件里的 `navLabel` */
const navItems: { key: PrimaryPage; icon: NavIcon }[] = [
    { key: "home", icon: HomeIcon },
    { key: "tasks", icon: AlignRightIcon },
    { key: "templates", icon: LayoutPanelTopIcon },
    { key: "about", icon: BadgeAlertIcon },
    { key: "settings", icon: SettingsIcon },
];

export function Sidebar() {
    const { entry, switchPrimary } = useNavigation();
    const [theme] = useTheme();
    // 后台那一趟查出「可装的新版、而你还没点开看过」才亮；点开那扇窗即灭（见 lib/update-store）
    const updateBadge = useUpdateBadge();
    const t = useT();
    // 表建在组件里、每格一条 `t(字面量)`：模块顶层建表会把词冻在首次加载的语言上
    const navLabel: Record<PrimaryPage, string> = {
        home: t("shell.home", "首页"),
        tasks: t("shell.tasks", "任务列表"),
        templates: t("shell.templates", "转换模板"),
        about: t("shell.about", "关于"),
        settings: t("common.settings", "设置"),
    };
    const dark = isDark();
    const themeBtn = useRef<HTMLButtonElement>(null);
    // 每点一次换一个 key 重播光环；不需要退场，终态本身就是透明
    const [pulse, setPulse] = useState(0);

    /** 快切只两态：点下去即锁定浅/深，因此读屏文案先把「这会离开跟随系统」说破（按钮无图标外的可见文字，靠它当名字） */
    const next = dark ? "light" : "dark";
    const shadeOf = (mode: "light" | "dark") => (mode === "light" ? t("shell.light", "亮色") : t("common.dark", "深色"));
    const themeLabel =
        theme === "system"
            ? t("shell.following-system", "跟随系统（当前{{shade}}）· 点击锁定为{{next}}", {
                  shade: shadeOf(dark ? "dark" : "light"),
                  next: shadeOf(next),
              })
            : t("shell.switch-shade", "切换到{{shade}}", { shade: shadeOf(next) });

    /** 当前一级页：栈顶是一级则取自身，二级则回溯到所属一级 */
    const activePrimary: PrimaryPage =
        entry.key in SECONDARY_OWNER
            ? SECONDARY_OWNER[entry.key as keyof typeof SECONDARY_OWNER]
            : (entry.key as PrimaryPage);

    // 「确认到达」：图标在换页落定那一下逐笔长出来，而不是鼠标压到图标才动。五颗都常驻挂载
    // （只有胶囊会滑），所以必须走命令式手柄；冷启动那一趟不播（首帧交给启动页与入场曲线）。
    // 自己收闸而不靠 MotionConfig：它管 transform 类，`pathLength`/`x1` 这类 SVG 属性不在降级范围内。
    const reduced = useReducedMotion();
    const iconRefs = useRef<Partial<Record<PrimaryPage, NavIconHandle | null>>>({});
    const arrivedFrom = useRef<PrimaryPage | null>(null);
    useEffect(() => {
        const from = arrivedFrom.current;
        arrivedFrom.current = activePrimary;
        if (from === null || from === activePrimary || reduced) return;
        iconRefs.current[activePrimary]?.startAnimation();
    }, [activePrimary, reduced]);

    return (
        // v5：侧栏全高、半透明白浮在氛围场上（材质令牌见 App.css「侧栏材质」段）。
        // 顶部的 BrandRow 与右侧标题栏同一行高（40）：各管各的拖拽区，拖左上角 = 拖品牌行。
        <aside
            data-tauri-drag-region
            className="w-[var(--side-w)] shrink-0 flex flex-col px-3 pb-4 border-r border-[var(--side-border)] bg-[var(--side-bg)] backdrop-blur-[var(--side-blur)]"
        >
            {/* 品牌行：Logo + SideShift + 预发布徽章（与标题栏同高 40，整行是拖拽区） */}
            <div
                data-tauri-drag-region
                className="h-10 shrink-0 flex items-center gap-2 px-3 select-none"
            >
                <Logo className="size-5" />
                <span className="font-heading text-base font-bold tracking-tight text-text-1">
                    SideShift
                </span>
                {/* 预发布徽章：文案由版本号推导（见 api 的 PRERELEASE_BADGE），稳定版整枚不渲染。
                    行高必须写死：html 的 24px 行高会继承进来，不锁就把 9px 字撑成 28px 高的胶囊。 */}
                {PRERELEASE_BADGE && (
                    <span className="inline-flex h-4 shrink-0 items-center rounded-full bg-accent-dim px-[7px] text-[9px] font-semibold leading-none tracking-[0.04em] text-accent">
                        {PRERELEASE_BADGE}
                    </span>
                )}
            </div>

            <div className="h-5" />

            {/* 导航：行 36 高 r8；激活 = 白卡胶囊 + 极淡投影 + accent 字（600），
                未激活回到 400 字重（v5 把导航字重拉开成 600/400 两档）。
                淡底交给一颗共享胶囊（layoutId），切分类时它从旧行滑到新行——跨行也滑，不瞬移换色。
                悬停带不走全站那条 $surface-2：侧栏面是半透白、压着场的冷光，中性灰贴上去等于没贴（判据见 App.css「侧栏材质」段的 --nav-hover） */}
            <nav className="flex flex-col gap-1">
                {navItems.map((item) => {
                    const Icon = item.icon;
                    const active = activePrimary === item.key;
                    return (
                        <button
                            key={item.key}
                            onClick={() => switchPrimary(item.key)}
                            className={cn(
                                "relative w-full h-9 rounded-md px-3 text-[13px]",
                                HOVER_FILL,
                                active
                                    ? "font-semibold text-accent"
                                    : "font-normal text-text-2 hover:bg-[var(--nav-hover)] hover:text-text-1"
                            )}
                        >
                            {active && (
                                <motion.span
                                    layoutId="nav-pill"
                                    transition={PILL_SLIDE}
                                    className="absolute inset-0 rounded-md bg-[var(--nav-pill)] shadow-[var(--nav-pill-shadow)]"
                                />
                            )}
                            <span className="relative z-[1] flex h-full items-center gap-2.5">
                                {/* `size` 是图形像素（站内默认 28，这里按基准收进 16），`className` 只套在外层那圈 div 上 */}
                                <Icon
                                    size={16}
                                    className="shrink-0"
                                    ref={(el) => {
                                        iconRefs.current[item.key] = el;
                                    }}
                                />
                                <span>{navLabel[item.key]}</span>
                            </span>
                        </button>
                    );
                })}
            </nav>

            <div className="flex-1" />

            {/* 底部：版本号 + 主题切换（32×32 纱面 + 描边，材质令牌见 App.css「侧栏材质」段）。
                `relative` 是给提示区当定位参照的：提示区挂在这一行的上方边缘（`bottom-full`），
                彻底不进侧栏的文档流——它长多高、什么时候塌掉都不该动到别的控件。
                顺带把 popLayout 退场件的位置参照也收在这圈里（原先侧栏没有 positioned 祖先，
                被抽成 absolute 的退场卡片是挂到整窗外壳那个 `relative` 上的）。 */}
            <div className="relative flex items-center justify-between pt-3 pr-0 pb-3 pl-3">
                <NotificationStack />
                {/* 版本号：更新角标挂在这里（原来在设置项上，挪过来离「这是哪一版」更近）。
                    有角标才可点（点开更新窗并记一次看过）；Tip 挂项目自己的气泡——
                    它贴着窗底，side 必须是 top，不然会把气泡算给滚动祖先撑出滚动条 */}
                <button
                    type="button"
                    aria-label={updateBadge ? t("settings.update-found", "发现新版本") : undefined}
                    className={cn(
                        "relative flex items-center gap-1.5 font-mono text-[11px] text-text-3 transition-colors",
                        updateBadge ? TIP_TRIGGER + " cursor-pointer hover:text-text-1" : "cursor-default",
                    )}
                    onClick={() => {
                        if (updateBadge) openUpdate();
                    }}
                >
                    <Tip
                        label={t("settings.update-found", "发现新版本")}
                        side="top"
                        align="start"
                    />
                    {/* 角标钉在文字右上角（上标位）：外层 relative 只圈文字本身，
                        点的位置跟字号走而不跟按钮框走 */}
                    <span className="relative">
                        v{VERSION_CORE}
                        <AnimatePresence initial={false}>
                            {updateBadge && (
                                <motion.span
                                    key="update-dot"
                                    initial={{ opacity: 0, scale: 0.5 }}
                                    animate={{ opacity: 1, scale: 1 }}
                                    exit={{ opacity: 0, scale: 0.5 }}
                                    transition={{ duration: 0.16, ease: [0.16, 1, 0.3, 1] }}
                                    className="absolute -right-2 -top-1.5 size-1.5 rounded-full bg-emerald"
                                />
                            )}
                        </AnimatePresence>
                    </span>
                </button>
                <button
                    ref={themeBtn}
                    onClick={() => {
                        setPulse((n) => n + 1);
                        // 圆心取这枚按钮：颜色从手指落点铺开才读得出因果（见 switchTheme 的时序说明）
                        switchTheme(next, themeBtn.current);
                    }}
                    aria-label={themeLabel}
                    className={cn(
                        "relative size-8 rounded-lg flex items-center justify-center text-text-2 hover:text-text-1",
                        "border border-[var(--foot-btn-border)] bg-[var(--foot-btn-bg)] shadow-[var(--foot-btn-shadow)]",
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
