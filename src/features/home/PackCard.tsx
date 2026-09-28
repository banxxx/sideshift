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
import { tSource, useT } from "@/lib/i18n";
import { Btn, Divider, MetaCell, Panel, ToneChip, type Tone } from "@/components/ui";

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

export function PackCard({
    manifest,
    status,
    error,
    fileName,
    onChangeFile,
    onPrimary,
}: PackCardProps) {
    const t = useT();
    // 状态 → 芯片色调/图标/文案（表建在渲染里：顶层建会把 label 冻在首次加载的语言上）
    const badgeTable: Record<PackCardStatus, { label: string; tone: Tone; icon: typeof Check }> = {
        parsing: { label: t("lib.parsing", "解析中"), tone: "gold", icon: RefreshCw },
        ready: { label: t("home.detected", "已检测"), tone: "emerald", icon: Check },
        converting: { label: t("common.converting", "转换中"), tone: "gold", icon: RefreshCw },
        error: { label: t("home.parse-failed", "解析失败"), tone: "redstone", icon: X },
    };
    const badge = badgeTable[status];
    const parsing = status === "parsing";
    const failed = status === "error";
    /** 读数区（分隔线 + 四格）该不该摆：解析中要摆骨架，有 manifest 要摆真值。
     *  解析失败且没有任何读数时四格全是「–」——纯噪音，撤掉。
     *  这张卡的外框由 HomePage 那一行的定高（h-[clamp(256px,36vh,288px)]）撑着，而静止态
     *  内容就已经贴着 288：失败文案那 2 行没有地方去，就会画到下面的轨道上面去（定高容器
     *  不会把兄弟往下推，只会溢出叠印）。撤掉 128px 的空读数区才是把预算还给文案。 */
    const showMeta = parsing || !!manifest;

    return (
        <Panel className="min-w-0 flex-1">
            {/* 头部：标题 + 状态芯片 */}
            <div className="flex w-full items-center justify-between gap-3">
                <span className="text-[12px] leading-[18px] font-semibold text-text-2">
                    {t("home.detected-modpack", "检测到的整合包")}
                </span>
                <ToneChip tone={badge.tone} icon={badge.icon}>
                    {badge.label}
                </ToneChip>
            </div>

            {error && (
                /*i18n:
                    解析失败
                    缺少 modrinth.index.json，不是有效的 Modrinth 整合包
                    modrinth.index.json 缺少 dependencies.minecraft（无法确定 Minecraft 版本）
                    整合包中没有任何模组文件（mods 目录为空）
                    mods 目录中没有任何 .jar 模组文件
                    未找到 mods 目录，不是可识别的整合包（推荐直接使用 .mrpack）
                    backend.cf-empty-pack=这份 CurseForge 清单没有声明任何模组，包里也没有 jar 字节
                    暂不支持 .7z 格式，请先解压为 zip 或改用 .mrpack
                */
                <p className="text-[12px] leading-[16px] font-normal text-redstone">{tSource(error)}</p>
            )}

            {/* 文件名行 */}
            <div className="flex min-w-0 items-center gap-2">
                <Archive className="size-4 shrink-0 text-amethyst" />
                <span className="truncate font-mono text-[13px] leading-[20px] font-medium text-text-1">
                    {truncateMiddle(manifest?.fileName ?? fileName ?? t("home.awaiting-file", "等待选择文件…"), 30)}
                </span>
            </div>

            {showMeta && (
                <>
                    <Divider />

                    {/* 四格元数据：解析中显示骨架条 */}
                    <div className="flex flex-col gap-2">
                        <div className="flex gap-2">
                            <MetaCell
                                label={t("home.loader", "加载器")}
                                value={manifest?.loader}
                                skeleton={parsing}
                                valueClass="text-diamond"
                            />
                            <MetaCell
                                label="Minecraft"
                                value={manifest?.mcVersion}
                                skeleton={parsing}
                            />
                        </div>
                        <div className="flex gap-2">
                            <MetaCell
                                label={t("home.mod-count", "模组数量")}
                                value={manifest ? t("common.count", "{{count}} 个", { count: manifest.modCount }) : undefined}
                                skeleton={parsing}
                            />
                            <MetaCell
                                label={t("home.pack-size", "包体积")}
                                value={manifest ? formatSize(manifest.sizeBytes) : undefined}
                                skeleton={parsing}
                            />
                        </div>
                    </div>
                </>
            )}

            {/* 底部操作：错误态只留"重新选择"；其余态为 更换文件（描边）+ 主按钮 等宽平分 */}
            <div className="mt-auto flex w-full gap-2">
                {!failed && (
                    <Btn className="flex-1 px-3.5" onClick={onChangeFile}>
                        {t("home.change-file", "更换文件")}
                    </Btn>
                )}
                <Btn
                    variant="primary"
                    className="flex-1"
                    disabled={parsing}
                    onClick={failed ? onChangeFile : onPrimary}
                >
                    {parsing ? t("home.parsing", "解析中…") : failed ? t("common.re-select", "重新选择") : t("home.set-convert", "配置并转换")}
                    {!parsing && !failed && <ChevronRight className="size-[13px]" />}
                </Btn>
            </div>
        </Panel>
    );
}
