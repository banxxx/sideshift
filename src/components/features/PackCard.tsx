/**
 * 已选整合包信息卡（SS.pen Home `OFbnE` PackCard）
 *
 * 展示解析结果（加载器/MC 版本/模组数/体积），右上角芯片随状态切换：
 * parsing=金色"解析中" / ready=绿色"已检测" / converting=金色"转换中" / error=红色"解析失败"。
 * 解析失败时错误文案显示在 meta-head 下方（Errors 设计稿的"重新选择"出口即本卡主按钮）。
 */
import { Archive, Check, ChevronRight, RefreshCw, X } from "lucide-react";
import type { PackManifest } from "@/lib/types";
import { formatSize, truncateMiddle } from "@/lib/format";
import { Btn, Divider, MetaCell, Panel, ToneChip, type Tone } from "@/components/design/ui";

export type PackCardStatus = "parsing" | "ready" | "converting" | "error";

interface PackCardProps {
    manifest: PackManifest | null;
    status: PackCardStatus;
    error?: string | null;
    /** 解析失败时仍要展示的文件名（manifest 为 null 的兜底） */
    fileName?: string;
    onChangeFile: () => void;
    onPrimary: () => void;
}

/** 状态 → 芯片色调/图标/文案 */
const BADGE: Record<PackCardStatus, { label: string; tone: Tone; icon: typeof Check }> = {
    parsing: { label: "解析中", tone: "gold", icon: RefreshCw },
    ready: { label: "已检测", tone: "emerald", icon: Check },
    converting: { label: "转换中", tone: "gold", icon: RefreshCw },
    error: { label: "解析失败", tone: "redstone", icon: X },
};

export function PackCard({
    manifest,
    status,
    error,
    fileName,
    onChangeFile,
    onPrimary,
}: PackCardProps) {
    const badge = BADGE[status];
    const parsing = status === "parsing";
    const failed = status === "error";

    return (
        <Panel className="min-w-0 flex-1">
            {/* 头部：标题 + 状态芯片 */}
            <div className="flex w-full items-center justify-between gap-3">
                <span className="text-[12px] leading-[18px] font-semibold text-text-2">
                    检测到的整合包
                </span>
                <ToneChip tone={badge.tone} icon={badge.icon}>
                    {badge.label}
                </ToneChip>
            </div>

            {error && (
                <p className="text-[12px] leading-[16px] font-normal text-redstone">{error}</p>
            )}

            {/* 文件名行 */}
            <div className="flex min-w-0 items-center gap-2">
                <Archive className="size-4 shrink-0 text-amethyst" />
                <span className="truncate font-mono text-[13px] leading-[20px] font-medium text-text-1">
                    {truncateMiddle(manifest?.fileName ?? fileName ?? "等待选择文件…", 30)}
                </span>
            </div>

            <Divider />

            {/* 四格元数据：解析中显示骨架条 */}
            <div className="flex flex-col gap-2">
                <div className="flex gap-2">
                    <MetaCell
                        label="加载器"
                        value={manifest?.loader}
                        skeleton={parsing}
                        valueClass="text-diamond"
                    />
                    <MetaCell label="Minecraft" value={manifest?.mcVersion} skeleton={parsing} />
                </div>
                <div className="flex gap-2">
                    <MetaCell
                        label="模组数量"
                        value={manifest ? `${manifest.modCount} 个` : undefined}
                        skeleton={parsing}
                    />
                    <MetaCell
                        label="包体积"
                        value={manifest ? formatSize(manifest.sizeBytes) : undefined}
                        skeleton={parsing}
                    />
                </div>
            </div>

            {/* 底部操作：错误态只留"重新选择"；其余态为 更换文件（描边）+ 主按钮 等宽平分 */}
            <div className="mt-auto flex w-full gap-2">
                {!failed && (
                    <Btn className="flex-1 px-3.5" onClick={onChangeFile}>
                        更换文件
                    </Btn>
                )}
                <Btn
                    variant="primary"
                    className="flex-1"
                    disabled={parsing}
                    onClick={failed ? onChangeFile : onPrimary}
                >
                    {parsing ? "解析中…" : failed ? "重新选择" : "配置并转换"}
                    {!parsing && !failed && <ChevronRight className="size-[13px]" />}
                </Btn>
            </div>
        </Panel>
    );
}
