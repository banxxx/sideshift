/**
 * 「发现新版本」弹窗：把后端算完的结论原样摊开——订阅的渠道、发布时间、release 正文、产物清单，
 * 以及走不通一键更新时的那句原因。缺件**不报错**，只退回「打开发布页」：
 * 一个点了必然失败的按钮比没有按钮更糟。
 * 安装那颗钮要等换文件那条链路真的接上（父级传 `onInstall`）才出现。
 */
import { ExternalLink, FileArchive, FileKey2, Package, Rocket, ShieldAlert } from "lucide-react";
import { Btn, ListRow, MetaCell, ModalShell, NoteRow, SectionTitle, TagChip } from "@/components/ui";
import { channelLabel, formatSize, formatStamp } from "@/lib/format";
import { t, useT } from "@/lib/i18n";
import { openExternal } from "@/lib/api";
import type { UpdateAssetKind, UpdateBlocked, UpdateInfo } from "@/lib/types";
import { cn } from "@/lib/utils";

/** 产物种类 → 图标：认不出的一律通用压缩包，不猜 */
function assetIcon(kind: UpdateAssetKind) {
    switch (kind) {
        case "package":
            return Package;
        case "signature":
            return FileKey2;
        default:
            return FileArchive;
    }
}

/** 产物种类 → 标签文案（表建在函数里，与 evidenceLabel 同一条 i18n 规矩） */
function assetKindLabel(kind: UpdateAssetKind): string {
    return {
        package: t("update.kind-package", "安装包"),
        signature: t("update.kind-signature", "签名"),
        portable: t("update.kind-portable", "便携包"),
        other: t("update.kind-other", "其它"),
    }[kind];
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

export function UpdateDialog({
    info,
    onClose,
    onInstall,
}: {
    /** null 时不渲染：父级只在「确实有新版本」时才打开这扇窗 */
    info: UpdateInfo | null;
    onClose: () => void;
    /** 应用内更新的入口；没接上这条链路的阶段一律不传，按钮就不出现 */
    onInstall?: () => void;
}) {
    const t = useT();
    if (!info) return null;

    const canInstall = info.downloadable && onInstall !== undefined;

    return (
        <ModalShell
            open
            onClose={onClose}
            persistent
            width={560}
            height={440}
            icon={Rocket}
            title={`v${info.latest ?? info.current}`}
            titleTag={
                <TagChip tone={info.channel === "beta" ? "gold" : "muted"}>
                    {channelLabel(info.channel)}
                </TagChip>
            }
            sub={t("update.sub-current", "当前 v{{current}}", { current: info.current })}
            footerNote={
                info.blocked
                    ? blockedLabel(info.blocked)
                    : canInstall
                      ? t("update.footer-ready", "更新会先校验签名，再替换并重启")
                      : t("update.footer-release", "打开发布页即可下载这一版")
            }
            footerActions={
                <>
                    <Btn size="sm" variant="ghost" onClick={onClose}>
                        {t("common.close", "关闭")}
                    </Btn>
                    {info.releaseUrl && (
                        <Btn
                            size="sm"
                            icon={ExternalLink}
                            onClick={() => void openExternal(info.releaseUrl!)}
                        >
                            {t("update.open-release", "打开发布页")}
                        </Btn>
                    )}
                    {canInstall && (
                        <Btn size="sm" variant="primary" icon={Rocket} onClick={onInstall}>
                            {t("update.install-now", "立即更新")}
                        </Btn>
                    )}
                </>
            }
        >
            <div className="flex min-h-0 flex-1 flex-col gap-3">
                <div className="flex gap-2">
                    <MetaCell label={t("update.meta-current", "当前版本")} value={`v${info.current}`} />
                    <MetaCell label={t("update.meta-latest", "最新版本")} value={info.latest ? `v${info.latest}` : "—"} />
                    <MetaCell label={t("update.meta-published", "发布时间")} value={stampOf(info.publishedAt)} />
                </div>

                <div className="list-scroll flex min-h-0 flex-1 flex-col gap-3 overflow-auto">
                    <div className="flex flex-col gap-1.5">
                        <SectionTitle>{t("update.notes", "更新说明")}</SectionTitle>
                        {info.notes ? (
                            <p className="select-text whitespace-pre-wrap text-[12px] leading-[18px] font-normal text-text-2">
                                {info.notes}
                            </p>
                        ) : (
                            <span className="text-[12px] leading-[18px] font-normal text-text-3">
                                {t("update.no-notes", "这条发布没有写说明")}
                            </span>
                        )}
                    </div>

                    <div className="flex flex-col gap-1.5">
                        <SectionTitle>{t("update.assets", "更新产物")}</SectionTitle>
                        {info.assets.length === 0 ? (
                            <span className="text-[12px] leading-[18px] font-normal text-text-3">
                                {t("update.no-assets", "这条发布没有列出产物")}
                            </span>
                        ) : (
                            <div className="flex flex-col gap-1">
                                {info.assets.map((a) => {
                                    const Icon = assetIcon(a.kind);
                                    return (
                                        // 不挂 Tip：气泡在 .list-scroll 这个 overflow-auto 盒里，贴底那行会被裁掉
                                        <ListRow key={a.name} className="bg-surface-2">
                                            <Icon
                                                className={cn(
                                                    "size-3.5 shrink-0",
                                                    a.trusted ? "text-text-2" : "text-text-3"
                                                )}
                                            />
                                            <span
                                                className={cn(
                                                    "min-w-0 flex-1 truncate font-mono text-[11px] leading-[16px]",
                                                    a.trusted ? "text-text-1" : "text-text-3"
                                                )}
                                            >
                                                {a.name}
                                            </span>
                                            <TagChip tone={a.trusted ? undefined : "muted"} square>
                                                {assetKindLabel(a.kind)}
                                            </TagChip>
                                            <span className="shrink-0 font-mono text-[11px] leading-[16px] font-medium text-text-3">
                                                {formatSize(a.size)}
                                            </span>
                                        </ListRow>
                                    );
                                })}
                            </div>
                        )}
                    </div>

                    {/* 宿主不可信是安全判定，不是「缺个文件」：在清单下面再落一句，别只靠压灰 */}
                    {info.blocked === "untrusted-host" && (
                        <NoteRow icon={ShieldAlert} tone="gold">
                            {blockedLabel("untrusted-host")}
                        </NoteRow>
                    )}
                </div>
            </div>
        </ModalShell>
    );
}

/** GitHub 的时间串 → 本地完整时刻；认不出格式就原样显示，不替它编一个日期 */
function stampOf(iso: string | null): string {
    if (!iso) return "—";
    const d = new Date(iso);
    return Number.isNaN(d.getTime()) ? iso : formatStamp(d);
}
