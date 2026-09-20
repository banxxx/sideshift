/**
 * 当前选包状态（App 级会话 store）
 *
 * 原先是 HomePage 的组件局部 hook，切侧栏标签即卸载丢失——检测完成但
 * 未开始转换的包会被重置回 idle，而转换中的包却能记住（useActiveTask 从
 * 任务存储恢复），行为不一致。提升到 App 级后，侧栏切换不再打断工作流。
 *
 * 刻意只存内存：应用重启回到全新 idle（上次磁盘上的包可能已被移动/删除）。
 */
import { createContext, useCallback, useContext, useState, type ReactNode } from "react";
import * as api from "@/lib/api";
import type { PackManifest } from "@/lib/types";

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
}

const Ctx = createContext<PackStore | null>(null);

export function PackStoreProvider({ children }: { children: ReactNode }) {
    const [manifest, setManifest] = useState<PackManifest | null>(null);
    const [parsing, setParsing] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const [errorName, setErrorName] = useState<string | null>(null);
    // 初值取挂载时刻：应用重启后磁盘上恢复的历史任务一律算「上次构建」，不该出现在首页
    const [selectedAt, setSelectedAt] = useState(() => Date.now());

    const parse = useCallback(async (path: string) => {
        setParsing(true);
        setError(null);
        setErrorName(null);
        // 每次选包（含解析失败）都重新计时：旧任务从此与这套构建无关
        setSelectedAt(Date.now());
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
