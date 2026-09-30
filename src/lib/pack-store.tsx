/**
 * 当前选包状态（App 级会话 store）：侧栏切换不再打断「已检测未开始」的工作流。
 * 刻意只存内存：应用重启回到全新 idle（磁盘上的包可能已被移动/删除）。
 * 捎带存一份转换页草稿 planDraft：ConvertPage 只挂栈顶，卸载重进靠它回到离开时的方案与手改。
 */
import { createContext, useCallback, useContext, useRef, useState, type ReactNode } from "react";
import * as api from "@/lib/api";
import type { ModDisposition, PackManifest, PlanMod } from "@/lib/types";

/** 转换页「模组方案」那一套状态的快照（离开页面时写、进页面时读一次） */
export interface PlanDraft {
    plan: PlanMod[];
    extras: PlanMod[];
    overrides: Record<string, ModDisposition>;
    disabledIds: Set<string>;
    /** 离开时联网反查还在跑（= 页面那侧的 classifying）：回来要跟后端复核一次，别把「分类中」挂住 */
    onlinePending: boolean;
}

interface PackStore {
    manifest: PackManifest | null;
    parsing: boolean;
    error: string | null;
    /** 解析失败的文件名：错误态卡片仍要显示"是哪个包失败了" */
    errorName: string | null;
    /**
     * 本次选包的时刻（epoch 毫秒）。用于把「上次构建的结果」和「这一套新构建」分开：
     * 只有 createdAt ≥ selectedAt 的任务才属于当前选包。重新拖入同名整合包时
     * 文件名一样，靠文件名无法区分，必须靠时刻。
     */
    selectedAt: number;
    parse: (path: string) => Promise<void>;
    pickByDialog: () => Promise<void>;
    reset: () => void;
    /** 读当前包的转换页草稿；没有（首次进来/换过包/全局判定设置改过）返回 null。
     *  非响应式：只在挂载那一帧读，页面活着的时候没人替它改。 */
    getDraft: () => PlanDraft | null;
    /** 写草稿（转换页卸载时调）；当前没有选包则丢弃 */
    saveDraft: (value: PlanDraft) => void;
    /** 作废草稿 */
    clearDraft: () => void;
}

const Ctx = createContext<PackStore | null>(null);

export function PackStoreProvider({ children }: { children: ReactNode }) {
    const [manifest, setManifest] = useState<PackManifest | null>(null);
    const [parsing, setParsing] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const [errorName, setErrorName] = useState<string | null>(null);
    // 初值取挂载时刻：应用重启后磁盘上恢复的历史任务一律算「上次构建」，不该出现在首页
    const [selectedAt, setSelectedAt] = useState(() => Date.now());

    // 草稿用 ref 不用 state：它只在「离开转换页」那一刻写、在「进入转换页」那一帧读，
    // 中间没有任何视图该为它重渲染（换页时也就少一次 App 级 context 刷新）。
    // key = 文件名 + 选包时刻：重新拖入同名包是另一套构建，旧草稿必须对不上号。
    const draftRef = useRef<{ key: string; value: PlanDraft } | null>(null);
    const draftKey = manifest ? `${manifest.fileName}#${selectedAt}` : null;

    const getDraft = useCallback(() => {
        const d = draftRef.current;
        return d && d.key === draftKey ? d.value : null;
    }, [draftKey]);

    // saveDraft 的 key 取的是「调用那一刻所在渲染」的归属：转换页退场那一拍（约 140ms）之后
    // 用户才可能换包，届时这个闭包带着的是旧 key，写进去也只会被旧包那次读取对上——
    // 新包读不到，正是想要的。
    const saveDraft = useCallback(
        (value: PlanDraft) => {
            if (draftKey) draftRef.current = { key: draftKey, value };
        },
        [draftKey]
    );

    const clearDraft = useCallback(() => {
        draftRef.current = null;
    }, []);

    const parse = useCallback(async (path: string) => {
        setParsing(true);
        setError(null);
        setErrorName(null);
        // 每次选包（含解析失败）都重新计时：旧任务从此与这套构建无关
        setSelectedAt(Date.now());
        // 换包 = 旧草稿作废（key 本来也对不上，这里只是别把上一个包的清单留在内存里）
        draftRef.current = null;
        const fail = (msg: string) => {
            setManifest(null);
            setError(msg);
            setErrorName(path.split(/[\\/]/).pop() ?? path);
        };
        try {
            const m = await api.parsePack(path);
            if (m.parsed) {
                setManifest(m);
            } else {
                fail(m.error ?? "解析失败");
            }
        } catch (e) {
            fail(e instanceof Error ? e.message : String(e));
        } finally {
            setParsing(false);
        }
    }, []);

    const pickByDialog = useCallback(async () => {
        try {
            const path = await api.pickPackFile();
            if (path) await parse(path);
        } catch (e) {
            setError(e instanceof Error ? e.message : String(e));
        }
    }, [parse]);

    const reset = useCallback(() => {
        setManifest(null);
        setError(null);
        setErrorName(null);
        setParsing(false);
        setSelectedAt(Date.now());
        draftRef.current = null;
    }, []);

    return (
        <Ctx.Provider
            value={{
                manifest,
                parsing,
                error,
                errorName,
                selectedAt,
                parse,
                pickByDialog,
                reset,
                getDraft,
                saveDraft,
                clearDraft,
            }}
        >
            {children}
        </Ctx.Provider>
    );
}

/** 消费选包状态；必须包在 PackStoreProvider 内 */
export function usePackStore(): PackStore {
    const store = useContext(Ctx);
    if (!store) throw new Error("usePackStore 必须在 PackStoreProvider 内使用");
    return store;
}
