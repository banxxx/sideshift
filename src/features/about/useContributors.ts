/**
 * 鸣谢名单的取数节拍：本机快照先上屏（零网络）→ 后台对账一次 → 版本不同才换。
 *
 * 三档状态里只有 `empty`（一次数据都没拿到过）会在屏上留字。**对账失败是静默的**：
 * 快照还在屏上，用户看不出、也不需要看出这一轮没成功——这一块不抢全局提示区，
 * 一次网络抖动更不该在关于页弹错误。
 */
import { useCallback, useEffect, useRef, useState } from "react";
import * as api from "@/lib/api";
import type { AckList, AckPerson } from "@/lib/types";

type AckStatus = "pending" | "ready" | "empty";

export function useContributors() {
    const [people, setPeople] = useState<AckPerson[]>([]);
    const [status, setStatus] = useState<AckStatus>("pending");
    /** 已经上屏的那一份是哪一版；同一版不重排（否则每次进页整批卡各演一遍入场） */
    const version = useRef<string | null>(null);

    const take = useCallback((list: AckList) => {
        if (version.current === list.version) return;
        version.current = list.version;
        setPeople(list.people);
        setStatus(list.people.length ? "ready" : "empty");
    }, []);

    const pull = useCallback(async () => {
        try {
            take(await api.refreshAckList());
        } catch {
            // 手上什么都没有才落到空态；有快照就维持原样
            if (version.current === null) setStatus("empty");
        }
    }, [take]);

    useEffect(() => {
        let on = true;
        void api
            .loadAckSnapshot()
            .then((list) => {
                if (on && list?.people.length) take(list);
            })
            .catch(() => {
                // 读快照不会失败（坏文件在 Rust 侧就等于没有），留着只为不吞掉别的异常
            });
        void pull();
        return () => {
            on = false;
        };
    }, [take, pull]);

    return { people, status, retry: pull };
}
