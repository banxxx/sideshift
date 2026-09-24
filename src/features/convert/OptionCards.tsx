/**
 * Convert 左列的配置卡族：运行环境 / 客户端保留目录 / 启动参数 / 服务端设置。
 * 四张卡都只是 options 的分段视图，统一走 patch 覆写单包参数（离开页面即丢弃）。
 * 卡高与行距按设计稿定死，改动前先确认不会让卡片随内容抖动。
 */
import { Folder, Info, Plus, X } from "lucide-react";
import { useEffect, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import type {
    ConversionOptions,
    PackDirNode,
    PackManifest,
} from "@/lib/types";
import { RISE } from "@/lib/springs";
import {
    Btn,
    Divider,
    InlineRow,
    NoteRow,
    Panel,
    PanelHead,
    SearchSelect,
    Stepper,
    TextInput,
    Tip,
    TIP_TRIGGER,
    Toggle,
    HOVER_FILL,
    type SelectOption,
} from "@/components/ui";
import { cn } from "@/lib/utils";
import { findDirNode, DIFFICULTY_OPTIONS, GAMEMODE_OPTIONS } from "./constants";

/** 单包参数覆写入口（来自 ConvertPage 的 options state） */
type Patch = (p: Partial<ConversionOptions>) => void;

/* ---------------- 运行环境：三个版本下拉 + 启动脚本开关 ---------------- */

/* 各卡的 `readOnly` = 回看态：控件一律灰化成「一眼看出点不动」的样子
   （下拉与输入框压成凹底、开关与勾选框整件压一档、步进器只留值区），卡片不换排版——
   回看要核对的就是「当时那套配置长什么样」，所以压的是容器，数值与颜色保持读得清。 */

export function RuntimeEnvCard({
    options,
    patch,
    manifest,
    loader,
    mcOptions,
    loaderOptions,
    javaOptions,
    readOnly,
}: {
    options: ConversionOptions | null;
    patch: Patch;
    manifest: PackManifest;
    /** 加载器显示名（loaderLabel 结果，用于 Loader 下拉的标签） */
    loader: string;
    mcOptions: SelectOption[];
    loaderOptions: SelectOption[];
    javaOptions: SelectOption[];
    readOnly?: boolean;
}) {
    return (
        <Panel gap={14}>
            <PanelHead title="运行环境" />
            <div className="flex w-full gap-3">
                <SearchSelect
                    className="flex-1"
                    label="Minecraft 版本"
                    value={options?.mcVersion ?? manifest.mcVersion}
                    options={mcOptions}
                    searchable
                    searchPlaceholder="搜索版本…"
                    readOnly={readOnly}
                    onChange={(v) => patch({ mcVersion: v })}
                />
                <SearchSelect
                    className="flex-1"
                    label={`${loader} Loader`}
                    value={options?.loaderVersion ?? ""}
                    options={loaderOptions}
                    searchable
                    searchPlaceholder="搜索版本…"
                    readOnly={readOnly}
                    onChange={(v) => patch({ loaderVersion: v })}
                />
                <SearchSelect
                    className="flex-1"
                    label="Java 版本"
                    value={options?.javaVersion ?? ""}
                    options={javaOptions}
                    readOnly={readOnly}
                    onChange={(v) => patch({ javaVersion: v })}
                />
            </div>
            <Divider />
            <InlineRow label="生成启动脚本（start.sh / start.bat）">
                <Toggle
                    checked={options?.generateScripts ?? true}
                    readOnly={readOnly}
                    onChange={(v) => patch({ generateScripts: v })}
                />
            </InlineRow>
        </Panel>
    );
}

/* ---------------- 客户端保留目录：默认空态，「添加目录」弹窗主动勾选 ---------------- */

export function KeepDirsCard({
    options,
    packDirs,
    parsed = true,
    onPick,
    onRemove,
    readOnly,
}: {
    options: ConversionOptions | null;
    /** 目录勾选弹窗数据源（含递归文件数） */
    packDirs: PackDirNode[];
    /** 源包是否还能解析出目录树：回看旧任务时源文件可能已被移走，那时不能谎称「包内没有资源」 */
    parsed?: boolean;
    /** 只读视图（任务详情「方案」签）不传写入口：静态渲染下没有触发路径 */
    onPick?: () => void;
    /** 卡片行内移除单个保留目录（批量增删走 DirPickerModal 应用回写） */
    onRemove?: (path: string) => void;
    readOnly?: boolean;
}) {
    const keepDirs = options?.keepDirs ?? [];
    return (
        <Panel gap={14}>
            <PanelHead
                title="客户端保留目录"
                right={
                    readOnly ? undefined : (
                        <Btn size="sm" icon={Plus} disabled={packDirs.length === 0} onClick={() => onPick?.()}>
                            添加目录
                        </Btn>
                    )
                }
            />
            {keepDirs.length === 0 ? (
                <p className="w-full py-3 text-center text-[11px] text-text-3">
                    {readOnly
                        ? "该任务未保留任何包内目录"
                        : packDirs.length === 0
                          ? parsed
                                ? "包内未检测到可保留的目录（mods 之外没有资源文件）"
                                : "源包已不在原位置 · 该任务未保留任何包内目录"
                          : "尚未选择目录 · 点击上方「添加目录」从包内勾选"}
                </p>
            ) : (
                <div className="flex w-full flex-col gap-1">
                    <AnimatePresence initial={false} mode="popLayout">
                        {keepDirs.map((p) => {
                            const dir = findDirNode(packDirs, p);
                            return (
                                <motion.div
                                    key={p}
                                    layout
                                    initial={{ opacity: 0, y: -6 }}
                                    animate={{ opacity: 1, y: 0 }}
                                    exit={{ opacity: 0, y: -6 }}
                                    transition={RISE}
                                    className={`flex min-w-0 items-center gap-2.5 rounded-lg px-1.5 py-1 hover:bg-surface-2 ${HOVER_FILL}`}
                                >
                                    <Folder className="size-3.5 shrink-0 text-accent" />
                                    <span className="min-w-0 flex-1 truncate font-mono text-[12px] leading-[18px] font-medium text-text-1">
                                        {p}/
                                    </span>
                                    {dir && (
                                        <span className="shrink-0 font-mono text-[11px] leading-[16px] tabular-nums text-emerald">
                                            {dir.fileCount} 文件
                                        </span>
                                    )}
                                    {!readOnly && (
                                        <button
                                            onClick={() => onRemove?.(p)}
                                            aria-label="移除"
                                            className={cn(
                                                TIP_TRIGGER,
                                                "size-6 rounded-md text-text-3",
                                                "flex shrink-0 items-center justify-center",
                                                "hover:bg-redstone-dim hover:text-redstone",
                                                HOVER_FILL
                                            )}
                                        >
                                            <X className="size-3" />
                                            <Tip label="移除" />
                                        </button>
                                    )}
                                </motion.div>
                            );
                        })}
                    </AnimatePresence>
                </div>
            )}
            <NoteRow icon={Info}>
                {readOnly
                    ? "这些目录当时按原层级从源包复制进了服务端包（支持子目录）"
                    : parsed
                      ? "勾选的目录按原层级从源包复制到服务端（支持子目录）"
                      : "源包已不在原位置 · 无法浏览包内目录，已有条目仍可移除"}
            </NoteRow>
        </Panel>
    );
}

/* ---------------- 启动参数：内存步进器 + 开关 + JVM 参数扩展 ---------------- */

export function LaunchArgsCard({
    options,
    patch,
    readOnly,
}: {
    options: ConversionOptions | null;
    patch: Patch;
    readOnly?: boolean;
}) {
    return (
        <Panel gap={14}>
            <PanelHead title="启动参数" />
            <InlineRow label="服务器内存上限">
                <Stepper
                    value={Math.round((options?.memoryMb ?? 6144) / 1024)}
                    min={1}
                    max={32}
                    suffix="GB"
                    readOnly={readOnly}
                    onChange={(v) => patch({ memoryMb: v * 1024 })}
                />
            </InlineRow>
            <InlineRow label="无界面模式启动（--nogui）">
                <Toggle
                    checked={options?.nogui ?? false}
                    readOnly={readOnly}
                    onChange={(v) => patch({ nogui: v })}
                />
            </InlineRow>
            <InlineRow label="自动写入 eula=true（同意 Mojang EULA）">
                <Toggle
                    checked={options?.agreeEula ?? true}
                    readOnly={readOnly}
                    onChange={(v) => patch({ agreeEula: v })}
                />
            </InlineRow>
            <InlineRow label="Aikar's flags 优化参数组（G1GC 推荐）">
                <Toggle
                    checked={options?.useAikarFlags ?? false}
                    readOnly={readOnly}
                    onChange={(v) => patch({ useAikarFlags: v })}
                />
            </InlineRow>
            <InlineRow label="附加 JVM 参数">
                <TextInput
                    className="w-[240px]"
                    value={options?.extraJvmArgs ?? ""}
                    readOnly={readOnly}
                    onChange={(e) => patch({ extraJvmArgs: e.target.value })}
                    placeholder="原样拼入 start 脚本"
                    spellCheck={false}
                />
            </InlineRow>
        </Panel>
    );
}

/* ---------------- 服务端设置：server.properties 高频字段（包内自带同名文件时不覆盖） ---------------- */

export function ServerSettingsCard({
    options,
    patch,
    readOnly,
}: {
    options: ConversionOptions | null;
    patch: Patch;
    readOnly?: boolean;
}) {
    return (
        <Panel gap={14}>
            <PanelHead title="服务端设置" />
            <div className="flex w-full gap-3">
                <SearchSelect
                    className="flex-1"
                    label="游戏模式"
                    value={options?.gamemode ?? "survival"}
                    options={GAMEMODE_OPTIONS}
                    readOnly={readOnly}
                    onChange={(v) => patch({ gamemode: v as ConversionOptions["gamemode"] })}
                />
                <SearchSelect
                    className="flex-1"
                    label="难度"
                    value={options?.difficulty ?? "easy"}
                    options={DIFFICULTY_OPTIONS}
                    readOnly={readOnly}
                    onChange={(v) => patch({ difficulty: v as ConversionOptions["difficulty"] })}
                />
            </div>
            <div className="grid w-full grid-cols-2 gap-3">
                <Field label="服务器端口">
                    <NumField
                        value={options?.serverPort ?? 25565}
                        min={1}
                        max={65535}
                        readOnly={readOnly}
                        onCommit={(v) => patch({ serverPort: v })}
                    />
                </Field>
                <Field label="最大人数">
                    <NumField
                        value={options?.maxPlayers ?? 20}
                        min={1}
                        max={1000}
                        readOnly={readOnly}
                        onCommit={(v) => patch({ maxPlayers: v })}
                    />
                </Field>
            </div>
            <Field label="服务器描述（MOTD）">
                <TextInput
                    className="w-full"
                    value={options?.motd ?? ""}
                    readOnly={readOnly}
                    onChange={(e) => patch({ motd: e.target.value })}
                    placeholder="显示在服务器列表中的一行描述"
                    spellCheck={false}
                />
            </Field>
            <Field label="世界种子（留空 = 随机生成）">
                <TextInput
                    className="w-full"
                    value={options?.levelSeed ?? ""}
                    readOnly={readOnly}
                    onChange={(e) => patch({ levelSeed: e.target.value })}
                    placeholder="如 4045151867437057206"
                    spellCheck={false}
                />
            </Field>
            <Divider />
            <InlineRow label="正版验证（online-mode）">
                <Toggle
                    checked={options?.onlineMode ?? true}
                    readOnly={readOnly}
                    onChange={(v) => patch({ onlineMode: v })}
                />
            </InlineRow>
            <NoteRow icon={Info}>
                以上字段写入包内 server.properties；整合包自带该文件时保留原文件
            </NoteRow>
        </Panel>
    );
}

/* ---------------- 服务端设置卡的字段小件 ---------------- */

function Field({ label, children }: { label: string; children: React.ReactNode }) {
    return (
        <label className="flex min-w-0 flex-1 flex-col gap-1.5">
            <span className="text-[11px] leading-[16px] font-medium text-text-2">{label}</span>
            {children}
        </label>
    );
}

/** 数字输入：本地草稿允许瞬时空串/半成品，失焦时 clamp 提交回 options */
function NumField({
    value,
    min,
    max,
    readOnly,
    onCommit,
}: {
    value: number;
    min: number;
    max: number;
    readOnly?: boolean;
    onCommit: (v: number) => void;
}) {
    const [draft, setDraft] = useState(String(value));
    useEffect(() => setDraft(String(value)), [value]);
    return (
        <TextInput
            className="w-full"
            inputMode="numeric"
            value={draft}
            readOnly={readOnly}
            onChange={(e) => setDraft(e.target.value.replace(/\D/g, "").slice(0, 6))}
            onBlur={() => {
                const n = parseInt(draft, 10);
                const c = Number.isFinite(n) ? Math.min(max, Math.max(min, n)) : value;
                setDraft(String(c));
                onCommit(c);
            }}
        />
    );
}
