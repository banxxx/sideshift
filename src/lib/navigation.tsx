/**
 * SideShift 导航模型（对应 SS.pen v17 页面流转设计）
 *
 * - 一级页面（PrimaryPage）：侧栏常驻目的地（首页/任务列表/关于/设置），标题栏【无】返回按钮
 * - 二级页面（SecondaryPage）：从一级页面下钻进入（转换配置/任务详情），
 *   标题栏最小化左侧出现 undo-2 返回按钮 + 短竖线分隔，点击回退一层
 * - 实现方式：内存导航栈。navigate() 压栈，back() 弹栈，switchPrimary() 清栈重建
 * - 栈顶用 EntryFreezer 冻成「本层挂载时那一份」再发给页面，原因见该组件的注释
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
export type PrimaryPage = "home" | "tasks" | "about" | "settings";

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

/**
 * 本层页面挂载时的栈顶。页面读到的 entry 由它提供，而不是直接读实时栈顶。
 *
 * 换页是串行的（App.tsx 的 `mode="wait"`）：旧页还要在屏上演 140ms，栈顶却已经换到新页了。
 * context 是穿透这层快照的，于是退场中的二级页会拿新栈顶去解自己的参数——
 * 详情页于是读不到 taskId，在返回列表之前先宣布「任务不存在或已过期」，也就是用户看到的闪一下。
 */
const EntrySnapshotContext = createContext<StackEntry | null>(null);

/**
 * 包在一层页面外面，把传进来的栈顶钉死在挂载那一刻（useState 的初值只在首渲染取一次，
 * 之后 props 再怎么变都不跟着走）。退场那 140ms 里旧页照旧按自己那份参数演完。
 */
export function EntryFreezer({ entry, children }: { entry: StackEntry; children: ReactNode }) {
    const [frozen] = useState(entry);
    return (
        <EntrySnapshotContext.Provider value={frozen}>
            {children}
        </EntrySnapshotContext.Provider>
    );
}

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
    const frozen = useContext(EntrySnapshotContext);
    if (!ctx) {
        throw new Error("useNavigation 必须在 <NavigationProvider> 内使用");
    }
    return useMemo(
        () => (frozen ? { ...ctx, entry: frozen } : ctx),
        [ctx, frozen]
    );
}
