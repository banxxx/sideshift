/**
 * 第 3 页：完成
 *
 * 路径全部来自 Rust 回传的 outcome（实际写盘/查到的那些），不是第 1 页那份选择状态：
 * 目录被建歪了、偏好文件没写成，这一页得如实显示，而不是复述一遍用户点过的按钮。
 *
 * 左对齐两行摘要，不做居中打勾 hero：装完的人下一秒要找的是"程序在哪、数据在哪"，
 * 不是被告知"你成功了"。
 */
import { CheckBox } from "@/components/ui/Field";
import type { Outcome } from "./api";

export function DonePage({
    outcome,
    launch,
    onToggleLaunch,
    error,
}: {
    outcome: Outcome;
    launch: boolean;
    onToggleLaunch: (v: boolean) => void;
    error: string | null;
}) {
    return (
        <div className="page-in flex flex-col">
            <h1 className="text-[19px] leading-[26px] font-semibold text-text-1">
                SideShift 已安装
            </h1>
            <p className="mt-1 text-[12px] leading-[18px] text-text-2">
                可以直接开始转换整合包了。
            </p>

            <dl className="mt-4 flex flex-col gap-2 border-t border-stroke-soft pt-3.5">
                <Row label="程序位置" value={outcome.installedExe} />
                <Row label="数据目录" value={outcome.dataRoot} />
            </dl>

            {/* 卸载入口没换上不等于没装上：控制面板里照样卸得掉，只是那套界面是 NSIS 的。
                所以这行是灰的，不抢红——但也不能不报，人以后找的是"为什么它长得不一样" */}
            {outcome.uninstallNote && (
                <p className="mt-2 text-[11px] leading-[16px] text-text-3">
                    {outcome.uninstallNote}
                </p>
            )}

            <div className="mt-[18px] flex items-center gap-2">
                <CheckBox checked={launch} onChange={onToggleLaunch} />
                <span
                    onClick={() => onToggleLaunch(!launch)}
                    className="cursor-pointer text-[12px] leading-[18px] text-text-1"
                >
                    安装完成后启动 SideShift
                </span>
            </div>

            {error && (
                <p className="mt-2 text-[12px] leading-[18px] text-redstone">{error}</p>
            )}
        </div>
    );
}

function Row({ label, value }: { label: string; value: string }) {
    return (
        <div className="flex items-baseline gap-3">
            <dt className="w-[76px] shrink-0 text-[12px] leading-[18px] text-text-3">
                {label}
            </dt>
            <dd className="min-w-0 truncate font-mono text-[12px] leading-[18px] text-text-1">
                {value}
            </dd>
        </div>
    );
}
