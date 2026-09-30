/**
 * 导航模型：一级页（首页/任务列表/关于/设置，侧栏常驻，标题栏无返回）与二级页（下钻进入，标题栏有返回按钮）。
 * 实现是内存导航栈：navigate() 压栈，back() 弹栈，switchPrimary() 清栈重建。
 * 栈顶用 EntryFreezer 冻成「本层挂载时那一份」再发给页面，原因见该组件的注释。
 */
import {
    createContext,
    useCallback,
    useContext,
    useMemo,
    useRef,
    useState,
    type ReactNode,
} from "react";

/** 一级页面 key */
export type PrimaryPage = "home" | "tasks" | "templates" | "about" | "settings";

/**
 * 二级页面 key：报告与方案不再是独立页面，它们收在任务详情的页签里
 * （转换配置是唯一剩下的深页，且只有「新建转换」这一个写入口；模板编辑页同理，是模板列表的唯一写入口）
 */
export type SecondaryPage = "convert" | "task" | "template";

export type PageKey = PrimaryPage | SecondaryPage;

/** 二级页面 → 所属一级页面：用于侧栏高亮与返回兜底 */
export const SECONDARY_OWNER: Record<SecondaryPage, PrimaryPage> = {
    convert: "home",
    task: "tasks",
    template: "templates",
};

/**
 * 离开拦截器：页面（模板编辑页）拿它把「返回/切一级页」按下来，先问用户一句再走。
 * 收到 `proceed` = 这趟导航已经被拦下，页面在校验完（用户选「放弃修改」）后调它继续。
 * 返回 `false` = 不拦，导航照常发生。
 */
export type LeaveGuard = (proceed: () => void) => boolean;

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
    /**
     * 挂/摘离开拦截器（传 null 摘）。没挂 = 导航行为与从前一致，所以其它页面完全不受影响。
     * 页面必须在卸载时摘掉：拦截器活在一个 ref 里，React 不会替我们清。
     */
    setLeaveGuard: (guard: LeaveGuard | null) => void;
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

    // 离开拦截器存 ref：它跟着页面挂载/卸载走，不该为一次注册触发全应用重渲染，
    // 也不能进 setStack 的更新函数里读（更新函数会被 StrictMode 调两遍，拦截会问两句）
    const guardRef = useRef<LeaveGuard | null>(null);
    const setLeaveGuard = useCallback((guard: LeaveGuard | null) => {
        guardRef.current = guard;
    }, []);

    /** 统一出口：先问拦截器，它不拦才真的动栈 */
    const go = useCallback((run: () => void) => {
        const guard = guardRef.current;
        if (guard?.(run)) return;
        run();
    }, []);

    const navigate = useCallback(
        (key: PageKey, params?: Record<string, unknown>) =>
            setStack((s) => [...s, { key, params }]),
        []
    );

    const back = useCallback(
        () => go(() => setStack((s) => (s.length > 1 ? s.slice(0, -1) : s))),
        [go]
    );

    const switchPrimary = useCallback(
        (key: PrimaryPage) => go(() => setStack([{ key }])),
        [go]
    );

    const entry = stack[stack.length - 1];

    const value = useMemo<Navigation>(
        () => ({ entry, canGoBack: stack.length > 1, navigate, back, switchPrimary, setLeaveGuard }),
        [entry, stack.length, navigate, back, switchPrimary, setLeaveGuard]
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
