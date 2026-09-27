/**
 * 鸣谢名单的取数节拍：本机快照先上屏（零网络）→ **本次启动只后台对账一次** → 版本不同才换。
 *
 * 为什么不跟着进页走：这份数据几周一改，而关于页是可以来回切的。挂在页上＝每次进页一发请求，
 * 用户量一上来就是白烧的重复流量。现在的口径是「一次启动一次对账」，页内切走再回来只读本地快照
 * （那条零网络，可以每次读）。
 *
 * 三档状态里只有 `empty`（一次数据都没拿到过）会在屏上留字。**对账失败是静默的**：
 * 快照还在屏上，用户看不出、也不需要看出这一轮没成功——这一块不抢全局提示区，
 * 一次网络抖动更不该在关于页弹错误。失败之后本次启动不再自动重试，
 * 出口是空态里那颗「重新获取」（它走 `retry`，不受下面的闸门管）。
 */
import { useCallback, useEffect, useRef, useState } from "react";
import * as api from "@/lib/api";
import type { AckList, AckPerson } from "@/lib/types";

type AckStatus = "pending" | "ready" | "empty";

/** 表空的时候给同一个对象：每轮渲染造一个新的 `{}` 会让名单那一片卡片各白比对一次 props */
const NO_SKINS: Record<string, string> = {};

/** 本次启动是否已经发起过对账。模块级＝跟着 WebView 活一次，关掉应用再开自然复位。
 *  这是「请求次数」的唯一收口点：将来要改成「隔几小时一次」，也只在这里加一个时间戳，别散到调用方。
 *  注：dev 下 HMR 换掉这个模块会让它复位，所以开发时的节奏比真实用户更密。 */
let reconciled = false;

export function useContributors() {
    const [people, setPeople] = useState<AckPerson[]>([]);
    const [skins, setSkins] = useState<Record<string, string>>(NO_SKINS);
    const [status, setStatus] = useState<AckStatus>("pending");
    /** 已经上屏的那一份是哪一版；同一版不重排（否则每次进页整批卡各演一遍入场） */
    const version = useRef<string | null>(null);
    /** 查过皮肤的那批名字。名单没换人就不再问第二遍——哪怕这一版对账失败、哪怕切走再回来 */
    const skinKey = useRef("");

    const take = useCallback((list: AckList) => {
        if (version.current === list.version) return;
        version.current = list.version;
        setPeople(list.people);
        setStatus(list.people.length ? "ready" : "empty");
    }, []);

    const pull = useCallback(async () => {
        // 同步置位、排在任何 await 之前：StrictMode 的双挂载、以及快速切页时两次挂载紧挨着发生，
        // 都因此只会被算一次（放在 await 之后就变成「两发都还没落地 ⇒ 两发都放行」）
        reconciled = true;
        try {
            take(await api.refreshAckList());
        } catch {
            // 手上什么都没有才落到空态；有快照就维持原样
            if (version.current === null) setStatus("empty");
        }
    }, [take]);

    useEffect(() => {
        let on = true;
        // 快照每次进页都读：本地一次文件读、不走网络，而且「上一次启动存下来的那份」要立刻可见
        void api.loadAckSnapshot().then((list) => {
            if (on && list?.people.length) take(list);
        });
        if (!reconciled) void pull();
        return () => {
            on = false;
        };
    }, [take, pull]);

    /* 皮肤地址只跟着「屏上这批 minecraftId 的名字」走：名单没换人就不问第二遍。
       后端那一份 7 天的缓存是第二道闸门（那里省的是 Mojang 的额度），这一道省的是 IPC。 */
    useEffect(() => {
        const names = people.filter((p) => p.minecraftId).map((p) => p.name);
        const key = names.join("|");
        if (key === skinKey.current) return;
        skinKey.current = key;
        if (!names.length) {
            setSkins(NO_SKINS);
            return;
        }
        let on = true;
        void api
            .fetchAckSkins(names)
            .then((m) => {
                if (on) setSkins(m);
            })
            .catch(() => {
                // 与名单同一条口径：拿不到就当没有，卡片自己落回下一层，不在这一页报错
                if (on) setSkins(NO_SKINS);
            });
        return () => {
            on = false;
        };
    }, [people]);

    return { people, skins, status, retry: pull };
}
