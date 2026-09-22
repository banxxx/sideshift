/**
 * SideShift 导航模型（对应 SS.pen v17 页面流转设计）
 *
 * - 一级页面（PrimaryPage）：侧栏常驻目的地（首页/任务列表/设置），标题栏【无】返回按钮
 * - 二级页面（SecondaryPage）：从一级页面下钻进入（转换配置/任务详情），
 *   标题栏最小化左侧出现 undo-2 返回按钮 + 短竖线分隔，点击回退一层
 * - 实现方式：内存导航栈。navigate() 压栈，back() 弹栈，switchPrimary() 清栈重建
 */
import {
    createContext,
    useCallback,
    useContext,
    useMemo,
    useState,
    type ReactNode,
} from "react";

/** 一级页面 key */
export type PrimaryPage = "home" | "tasks" | "settings";

/**
 * 二级页面 key：报告与方案不再是独立页面，它们收在任务详情的页签里
 * （转换配置是唯一剩下的深页，且只有「新建转换」这一个写入口）
 */
export type SecondaryPage = "convert" | "task";

export type PageKey = PrimaryPage | SecondaryPage;

/** 二级页面 → 所属一级页面：用于侧栏高亮与返回兜底 */
export const SECONDARY_OWNER: Record<SecondaryPage, PrimaryPage> = {
    convert: "home",
    task: "tasks",
};

/** 导航栈条目：页面 key + 可选参数（如 task 页的 { taskId }） */
export interface StackEntry {
    key: PageKey;
    params?: Record<string, unknown>;
}

interface Navigation {
    /** 当前栈顶条目（即正在展示的页面） */
    entry: StackEntry;
    /** 栈深 > 1 时可返回（等价于"处于二级页面"） */
    canGoBack: boolean;
    /** 下钻到二级页面 */
    navigate: (key: PageKey, params?: Record<string, unknown>) => void;
    /** 返回上一层 */
    back: () => void;
    /** 切换一级页面：清空下钻栈（侧栏导航语义） */
    switchPrimary: (key: PrimaryPage) => void;
}

const NavigationContext = createContext<Navigation | null>(null);

/** 供 DEV 调试句柄读取的最新导航实例（见文件底部 __nav） */
const navSingleton: { current: Navigation | null } = { current: null };

export function NavigationProvider({ children }: { children: ReactNode }) {
    const [stack, setStack] = useState<StackEntry[]>([{ key: "home" }]);

    const navigate = useCallback(
        (key: PageKey, params?: Record<string, unknown>) =>
            setStack((s) => [...s, { key, params }]),
        []
    );

    const back = useCallback(
        () => setStack((s) => (s.length > 1 ? s.slice(0, -1) : s)),
        []
    );

    const switchPrimary = useCallback(
        (key: PrimaryPage) => setStack([{ key }]),
        []
    );

    const entry = stack[stack.length - 1];

    const value = useMemo<Navigation>(
        () => ({ entry, canGoBack: stack.length > 1, navigate, back, switchPrimary }),
        [entry, stack.length, navigate, back, switchPrimary]
    );

    navSingleton.current = value;

    return (
        <NavigationContext.Provider value={value}>
            {children}
        </NavigationContext.Provider>
    );
}

// 仅开发环境暴露导航实例，供浏览器手动验证二级页面/返回按钮
if (import.meta.env.DEV) {
    (window as unknown as Record<string, unknown>).__nav = {
        navigate: (key: PageKey, params?: Record<string, unknown>) =>
            navSingleton.current?.navigate(key, params),
        back: () => navSingleton.current?.back(),
        switchPrimary: (key: PrimaryPage) => navSingleton.current?.switchPrimary(key),
    };
}

export function useNavigation(): Navigation {
    const ctx = useContext(NavigationContext);
    if (!ctx) {
        throw new Error("useNavigation 必须在 <NavigationProvider> 内使用");
    }
    return ctx;
}
