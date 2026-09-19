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

    const parse = useCallback(async (path: string) => {
        setParsing(true);
        setError(null);
        setErrorName(null);
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
    }, []);

    return (
        <Ctx.Provider
            value={{ manifest, parsing, error, errorName, parse, pickByDialog, reset }}
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
