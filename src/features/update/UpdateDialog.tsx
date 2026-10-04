/**
 * 「发现新版本」弹窗：整扇窗只有一个会变的零件——右下角那颗钮。
 * 进度、校验、就位、失败都活在这颗钮的文字与填充里，加上标题下面那一行小字；不再另开进度条、提示盒与产物清单。
 * 缺件不报错：那颗钮换成「打开发布页」——一个点了必然失败的按钮比没有按钮更糟。
 */
import { ExternalLink, Rocket } from "lucide-react";
import type { ReactNode } from "react";
import { Btn, ModalShell } from "@/components/ui";
import { formatDate, formatSize, ratioPercent } from "@/lib/format";
import { errOf } from "@/lib/errors";
import { t, useT } from "@/lib/i18n";
import { openExternal } from "@/lib/api";
import type { UpdateBlocked, UpdateInfo, UpdateStatus } from "@/lib/types";

/** 这一轮办的是不是弹窗里这一版：后端两处落点（占位那一次写 tag 原文，定稿那一次写版本号），所以两边都去 `v` 再比 */
function isThisRound(status: UpdateStatus, tag: string | null): boolean {
    if (!status.version || !tag) return false;
    const strip = (s: string) => s.replace(/^v/, "");
    return strip(status.version) === strip(tag);
}

/**
 * 一键更新走不通的那句话。五种原因只有一档差别：便携版说的是本机「形态」，
 * missing-key 说的是**我们这个构建**没带可信根，其余三种才是这条发布自己缺件——
 * 缺件是发布侧的事，用户没做错什么，措辞因此不带指责。
 */
function blockedLabel(b: UpdateBlocked): string {
    return {
        portable: t("update.blocked-portable", "便携版要整目录手动替换，这次不做应用内更新"),
        // 这一句的责任在我们身上：产物配齐也装不了，因为没人能证明它没被动过
        "missing-key": t("update.blocked-missing-key", "这个构建没内置校验公钥，请在发布页手动下载"),
        "no-package": t("update.blocked-no-package", "这条发布没有签名安装包，请在发布页手动下载"),
        "no-signature": t("update.blocked-no-signature", "这条发布缺同名签名文件，暂不做应用内更新"),
        "untrusted-host": t("update.blocked-untrusted-host", "安装包或签名的下载地址不在可信宿主，暂不更新"),
    }[b];
}

/** GitHub 的时间串 → 「2026-10-02」；认不出格式就原样显示，不替它编一个日期 */
function stampOf(iso: string | null): string {
    if (!iso) return "";
    const d = new Date(iso);
    return Number.isNaN(d.getTime()) ? iso : formatDate(d);
}

/**
 * 钮内进度：一层从左边推进来的淡底，底下压一条 2px 实心线收边。
 * 它是量规不是按钮（点它没有下一个动作），所以 `disabled` 但不压灰——压灰读起来是「这条链路坏了」，
 * 而它说的是「正在跑」。`disabled:opacity-100` 顶掉 Btn 那条 60%（同类目，tailwind-merge 取后者）。
 */
function ProgressBtn({ pct, children }: { pct: number; children: ReactNode }) {
    return (
        <Btn
            size="sm"
            variant="primary"
            disabled
            className="relative overflow-hidden bg-accent-dim text-accent hover:opacity-100 disabled:opacity-100"
        >
            <span
                className="absolute inset-y-0 left-0 z-[1] bg-current opacity-[0.17]"
                style={{ width: `${pct}%` }}
            />
            <span className="absolute bottom-0 left-0 z-[1] h-[2px] bg-current" style={{ width: `${pct}%` }} />
            <span className="relative z-[2]">{children}</span>
        </Btn>
    );
}

export function UpdateDialog({
    info,
    status,
    onClose,
    onStart,
    onCancel,
    onInstall,
}: {
    /** null 时不渲染：父级只在「确实有新版本」时才打开这扇窗 */
    info: UpdateInfo | null;
    /** 取件那一段的生命周期；办的不是这一版时按「没有轮次」演 */
    status: UpdateStatus;
    onClose: () => void;
    /** 开始取件（下载 + 验签）；没传就不亮那颗「立即更新」 */
    onStart?: () => void;
    onCancel?: () => void;
    /** 应用内换文件的那条链路（P2）：没接上时「重启并安装」画定形但禁用 */
    onInstall?: () => void;
}) {
    const t = useT();
    if (!info) return null;

    const round = isThisRound(status, info.tag);
    const verifying = round && status.stage === "verifying";
    const fetching = round && (status.stage === "downloading" || verifying);
    // 「取消之后」不是一屏：立旗那一档，与那一轮收尾报回来的 canceled，都回第一屏
    const canceled =
        status.stage === "canceled" ||
        (status.stage === "failed" && (status.error ?? "").endsWith("update-canceled"));
    const ready = round && status.stage === "ready";
    const failed = round && status.stage === "failed" && !canceled;
    // 缺件那句只在「这一版还没开始办」时说：轮次一旦跑起来，它自己的成败更贴近用户此刻做的事
    const blocked = info.blocked !== null && !round;

    // 安装包 + 同名签名：这一对正是后端要落地的两个字节数
    const stagedBytes = info.assets
        .filter((a) => a.kind === "package" || a.kind === "signature")
        .reduce((n, a) => n + a.size, 0);
    const totalBytes = fetching || ready ? Math.max(status.total, stagedBytes) : stagedBytes;
    const pct = ratioPercent(status.downloaded, status.total);

    const sub = (() => {
        if (fetching)
            return verifying
                ? t("update.line-verifying", "正在校验签名…")
                : t("update.line-downloading", "正在下载 · {{got}} / {{total}}", {
                      got: formatSize(status.downloaded),
                      total: formatSize(status.total),
                  });
        if (ready)
            return t("update.line-ready", "已验签 · {{size}} · 当前 v{{current}}", {
                size: formatSize(totalBytes),
                current: info.current,
            });
        if (failed)
            // 种类码渲染好的那一句就长在这一行上，只补一句「没动过本机」
            return t("update.line-failed", "{{reason}}，本机版本没变", { reason: errOf(status.error ?? "") });
        if (blocked && info.blocked) return blockedLabel(info.blocked);
        return t("update.line-idle", "{{size}} · {{date}} · 当前 v{{current}}", {
            size: formatSize(totalBytes),
            date: stampOf(info.publishedAt),
            current: info.current,
        });
    })();

    const openRelease = () => void openExternal(info.releaseUrl!);
    const canStart = info.downloadable && info.tag !== null && onStart !== undefined;

    // 两颗钮永远各在其位：换的是文字与配色，不是位置
    const left = fetching ? (
        <Btn size="sm" variant="ghost" onClick={onCancel}>
            {t("common.cancel", "取消")}
        </Btn>
    ) : ready ? (
        <Btn size="sm" variant="ghost" onClick={onClose}>
            {t("update.later-again", "稍后")}
        </Btn>
    ) : failed ? (
        info.releaseUrl && (
            <Btn size="sm" variant="ghost" icon={ExternalLink} onClick={openRelease}>
                {t("update.open-release", "打开发布页")}
            </Btn>
        )
    ) : (
        <Btn size="sm" variant="ghost" onClick={onClose}>
            {t("update.later", "以后再说")}
        </Btn>
    );

    const right = fetching ? (
        <ProgressBtn pct={verifying ? 100 : pct}>
            {verifying
                ? t("update.btn-verifying", "校验中…")
                : t("update.btn-downloading", "下载中 · {{pct}}%", { pct })}
        </ProgressBtn>
    ) : ready ? (
        <Btn size="sm" variant="primary" disabled={onInstall === undefined} onClick={onInstall}>
            {t("update.restart-install", "重启并安装")}
        </Btn>
    ) : failed ? (
        <Btn size="sm" variant="primary" icon={Rocket} onClick={onStart}>
            {t("update.retry", "重试")}
        </Btn>
    ) : canStart ? (
        <Btn size="sm" variant="primary" icon={Rocket} onClick={onStart}>
            {t("update.install-now", "立即更新")}
        </Btn>
    ) : info.releaseUrl ? (
        <Btn size="sm" variant="primary" icon={ExternalLink} onClick={openRelease}>
            {t("update.open-release", "打开发布页")}
        </Btn>
    ) : (
        <Btn size="sm" variant="primary" onClick={onClose}>
            {t("common.close", "关闭")}
        </Btn>
    );

    return (
        <ModalShell
            open
            onClose={onClose}
            persistent
            plainFooter
            width={460}
            icon={Rocket}
            title={t("update.title-new", "更新到 v{{version}}", { version: info.latest ?? info.current })}
            sub={sub}
            subTone={failed ? "danger" : undefined}
            footerActions={
                <>
                    {left}
                    {right}
                </>
            }
        >
            {/* 说明在四档里都挂着：只有那一行小字和那颗钮在换，窗高不跟着跳 */}
            {info.notes && (
                <p className="select-text line-clamp-4 pb-2 text-[12px] leading-[18px] font-normal whitespace-pre-wrap text-text-2">
                    {info.notes}
                </p>
            )}
        </ModalShell>
    );
}
