/**
 * 应用更新的进程级数据源：弹窗挂在 Shell（换页不该把它卸掉）、角标在侧栏、触发点在设置页，三处读同一份。
 * 判据不在这儿：有没有新版、能不能一键装都由 Rust 侧一处算完，这里只搬结论与那一轮的生命周期。
 * 取件那一段本来就是进程级的（关窗、换页都不打断它），所以它早就不该住在某个页面的 state 里。
 */
import { useSyncExternalStore } from "react";
import * as api from "@/lib/api";
import { errOf } from "@/lib/errors";
import { t } from "@/lib/i18n";
import { notify } from "@/lib/notify";
import type { UpdateInfo, UpdateStatus } from "@/lib/types";

/** 没有轮次时的那一档（与 Rust 的 `UpdateStatus::idle()` 同形） */
const IDLE: UpdateStatus = {
    stage: "idle",
    version: null,
    downloaded: 0,
    total: 0,
    error: null,
};

export interface UpdateStore {
    /**
     * 最近一次检查的结论。`null` = 这一趟启动还没查出来过。
     * **关窗不清**：那一轮还在跑时，重开不该再敲一次网络
     */
    info: UpdateInfo | null;
    /** 手动那一趟在飞（设置页那颗钮的「检查中…」） */
    checking: boolean;
    /** 取件那一段（下载 / 校验 / 就位 / 失败） */
    status: UpdateStatus;
    open: boolean;
    /** 侧栏「设置」那颗的角标：只有「后台查出可装的新版、而你还没点开看过」才亮 */
    badge: boolean;
}

let s: UpdateStore = { info: null, checking: false, status: IDLE, open: false, badge: false };
const listeners = new Set<() => void>();

function set(patch: Partial<UpdateStore>) {
    s = { ...s, ...patch };
    for (const l of listeners) l();
}

function subscribe(cb: () => void): () => void {
    listeners.add(cb);
    return () => void listeners.delete(cb);
}

/** 整份快照（Shell 的弹窗与设置页那一行都吃它）。取件进度每跳一次它就换一次引用——那是画进度要的 */
export function useUpdate(): UpdateStore {
    return useSyncExternalStore(subscribe, () => s);
}

/** 只要角标那一个 bool：侧栏每帧重画跟下载字节数没关系 */
export function useUpdateBadge(): boolean {
    return useSyncExternalStore(subscribe, () => s.badge);
}

/**
 * 挂树时接一次：对齐上一轮办到哪一步、拿角标的初始值、接上两条事件。
 * 返回退订——StrictMode 会把挂载演两遍，事件订阅留双份就等于每个进度跳两次。
 */
export function initUpdate(): () => void {
    let alive = true;
    const stop: Array<() => void> = [];
    const keep = (p: Promise<() => void>) =>
        p
            .then((fn) => {
                if (alive) stop.push(fn);
                else fn();
            })
            .catch(() => {
                // 订阅失败与查不到同义：界面退回「没有轮次、角标不亮」，不替它编一条错误出来
            });
    keep(api.onUpdateProgress((st) => {
        if (alive) set({ status: st });
    }));
    keep(api.onUpdateAvailable((info) => {
        // 只点角标，不开窗：用户在忙别的的时候跳出一扇升级窗是打断
        if (alive) set({ info, badge: true });
    }));
    void api
        .updateStatus()
        .then((st) => {
            if (alive) set({ status: st });
        })
        .catch(() => {
            // 这一句只是「对齐」，拿不到就当没有轮次：真跑起来时事件照样会把它填上
        });
    void api
        .updateBadge()
        .then((info) => {
            if (alive && info) set({ info, badge: true });
        })
        .catch(() => {});
    return () => {
        alive = false;
        for (const fn of stop) fn();
    };
}

/**
 * 查一次（设置页那颗钮，不受 24 小时闸门——那道闸是给「用户没开口」的那些趟设的）。
 * 有新版本走弹窗：哪一版、多大、什么时候发的、能不能一键更新，这几件事侧栏那条一句话的横幅装不下；
 * 没有更新或查失败才回落到横幅。
 */
export async function runCheck(): Promise<void> {
    set({ checking: true });
    try {
        const info = await api.checkUpdate();
        // 手动这一趟后端把「看过」记在同一刻 ⇒ 角标一律灭（哪怕这次查出的是「没更新」：
        // 那条 release 被撤了就留着点，等于一颗指向空窗的点）
        set({ info, badge: false });
        if (info.hasUpdate) {
            set({ open: true });
        } else {
            notify(t("settings.already-date", "已是最新版本 v{{current}}", { current: info.current }), "success");
        }
    } catch (e) {
        // 网络不通、仓库还没发过 release 都会走到这里：只报错，不许顶着一个假的「已是最新」
        notify(t("settings.update-check", "检查更新失败：{{reason}}", { reason: errOf(e) }), "error");
    } finally {
        set({ checking: false });
    }
}

/** 打开这扇窗＝「看过」：角标当场灭，后端那本账记下这一刻，直到下一趟敲出新版本才重新亮 */
export function openUpdate(): void {
    set({ open: true, badge: false });
    api.markUpdateSeen();
}

export function closeUpdate(): void {
    set({ open: false });
}

/**
 * 开始取件（下载 + 验签）。**不等这句**：整轮都在这一个调用里，过程走 `update://progress`，
 * 它的返回值只当最后一锤。所以这里只负责把 reject 的那几种「没跑起来」说出来
 * （并发第二轮、这个构建没带公钥、tag 读不懂）——半途失败由弹窗那行红字说。
 */
export function startFetch(): void {
    const tag = s.info?.tag;
    if (!tag) return;
    void api
        .prepareUpdate(tag)
        .then((st) => set({ status: st }))
        .catch((e) => notify(t("settings.update-start", "没能开始下载：{{reason}}", { reason: errOf(e) }), "error"));
}

/** 取消这一轮：正在下就只立旗（半截由那一轮自己收走），已经定稿就把暂存收走。关窗不等于取消 */
export function cancelFetch(): void {
    void api
        .cancelUpdate()
        .then((st) => {
            set({ status: st });
            notify(t("settings.update-canceled", "已取消这次下载"), "info");
        })
        .catch((e) => notify(t("settings.update-cancel-failed", "取消没能生效：{{reason}}", { reason: errOf(e) }), "error"));
}

/**
 * 立即安装。**不等它成功**：成功那一路的最后一句是退出这个进程，窗口当场就没了，
 * 这里连返回值都寄不到。所以只说得出「没能开始安装」那一句——装成了没有要等下趟启动的提示
 * （出口在 `main.tsx`：读 `update_outcome` 那本账）。
 */
export function installUpdate(): void {
    void api.installUpdate().catch((e) =>
        notify(t("settings.update-install", "没能开始安装：{{reason}}", { reason: errOf(e) }), "error"),
    );
}
