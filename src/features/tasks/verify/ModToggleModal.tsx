/**
 * 验证副本的模组启停弹窗：动作只有改名——给 `mods/` 里那枚 jar 加或去 `.disabled`，交付包与转换记录都不动。
 * 任何一枚都能停，这里不做依赖裁决；停错了服务端起来就报错，报错原文在验证页的控制台里看。
 * 名单每次打开重拉一次：改的是磁盘，上一轮留下的状态得跟着这一轮走。
 */
import { useEffect, useMemo, useState } from "react";
import * as api from "@/lib/api";
import { errOf } from "@/lib/errors";
import { formatSize } from "@/lib/format";
import { useT } from "@/lib/i18n";
import { notify } from "@/lib/notify";
import type { VerifyModEntry } from "@/lib/types";
import {
    Btn,
    CheckBox,
    ListRow,
    ModalShell,
    SearchBox,
    SegTabs,
    Swap,
    TagChip,
} from "@/components/ui";
import { cn } from "@/lib/utils";

type Tab = "enabled" | "disabled";

export function ModToggleModal({
    open,
    onClose,
    taskId,
    onToggled,
}: {
    open: boolean;
    onClose: () => void;
    taskId: string;
    /** 改完一名：页面按快照重拉一次，计数与那行日志都从后端来，不在这里自己加减 */
    onToggled: () => void;
}) {
    const t = useT();
    const [rows, setRows] = useState<VerifyModEntry[]>([]);
    const [query, setQuery] = useState("");
    const [tab, setTab] = useState<Tab>("enabled");
    /** 一次只走一名：改名是磁盘动作，连着点会后一次盖前一次 */
    const [busy, setBusy] = useState(false);

    useEffect(() => {
        if (!open) return;
        let alive = true;
        setQuery("");
        setTab("enabled");
        void api
            .listVerifyMods(taskId)
            .then((list) => {
                if (alive) setRows(list);
            })
            .catch((e) => {
                if (alive) notify(errOf(e), "error");
            });
        return () => {
            alive = false;
        };
    }, [open, taskId]);

    const on = rows.filter((m) => m.enabled).length;
    const off = rows.length - on;

    const filtered = useMemo(() => {
        const q = query.trim().toLowerCase();
        return rows
            .filter((m) => (tab === "enabled" ? m.enabled : !m.enabled))
            .filter((m) => !q || m.name.toLowerCase().includes(q) || m.file.toLowerCase().includes(q));
    }, [rows, tab, query]);

    const toggle = async (m: VerifyModEntry) => {
        if (busy) return;
        setBusy(true);
        try {
            const res = await api.setVerifyModEnabled(taskId, m.file, !m.enabled);
            setRows(res.mods);
            onToggled();
        } catch (e) {
            notify(errOf(e), "error");
        } finally {
            setBusy(false);
        }
    };

    return (
        <ModalShell
            open={open}
            onClose={onClose}
            persistent
            width={640}
            height={468}
            title={t("verify.mods-title", "启用 / 停用模组")}
            sub={t("verify.mods-sub", "改名只落在验证副本，重新验证后生效。")}
            footerNote={
                rows.length > 0
                    ? t("verify.mods-count", "{{count}} 项 · 启用 {{on}} · 停用 {{off}}", {
                          count: rows.length,
                          on,
                          off,
                      })
                    : t("verify.mods-none", "副本里没有模组。")
            }
            footerActions={
                <Btn size="sm" className="px-3.5" onClick={onClose}>
                    {t("common.close", "关闭")}
                </Btn>
            }
        >
            <SearchBox
                value={query}
                onChange={setQuery}
                placeholder={t("verify.mods-search", "搜索模组名称…")}
                className="border border-stroke"
            />

            <div className="flex w-full shrink-0 justify-end">
                <SegTabs<Tab>
                    size="sm"
                    value={tab}
                    onChange={setTab}
                    items={[
                        { key: "enabled", label: t("verify.mods-tab-enabled", "已启用"), count: on },
                        { key: "disabled", label: t("verify.mods-tab-disabled", "已停用"), count: off },
                    ]}
                />
            </div>

            <div className="list-scroll -mx-1 flex min-h-0 flex-1 flex-col overflow-auto px-1">
                <Swap swapKey={tab} className="gap-2">
                    {filtered.map((m) => (
                        <ListRow
                            key={m.file}
                            className={cn(
                                "group/row cursor-pointer py-2.5 hover:bg-surface-2 hover:rounded-md",
                                busy && "pointer-events-none opacity-60"
                            )}
                            onClick={() => void toggle(m)}
                        >
                            {/* 勾选位不接 onChange：点击冒泡到整行，一行只留一个状态源 */}
                            <CheckBox checked={m.enabled} readOnly onChange={() => {}} />
                            <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                                <span className="truncate font-mono text-[12px] leading-[18px] font-medium text-text-1">
                                    {m.file}
                                </span>
                                {m.name !== m.file && (
                                    <span className="truncate text-[10px] leading-[14px] font-normal text-text-3">
                                        {m.name}
                                    </span>
                                )}
                            </span>
                            <TagChip square>{formatSize(m.sizeBytes)}</TagChip>
                        </ListRow>
                    ))}
                    {filtered.length === 0 && (
                        <span className="py-8 text-center text-[11px] text-text-3">
                            {rows.length === 0
                                ? t("verify.mods-none", "副本里没有模组。")
                                : t("verify.mods-no-match", "这一档没有匹配的模组。")}
                        </span>
                    )}
                </Swap>
            </div>
        </ModalShell>
    );
}
