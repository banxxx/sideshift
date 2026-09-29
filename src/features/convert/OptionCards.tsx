/**
 * Convert 左列的配置卡族：运行环境 / 客户端保留内容 / 启动参数 / 服务端设置。
 * 四张卡都只是 options 的分段视图，统一走 patch 覆写单包参数（离开页面即丢弃）。
 * 卡高与行距按设计稿定死，改动前先确认不会让卡片随内容抖动。
 */
import { AlertTriangle, FileText, Folder, Info, Plus, X, type LucideIcon } from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";
import { AnimatePresence, motion } from "motion/react";
import type {
    ConversionOptions,
    JavaInstall,
    JavaProbe,
    PackDirTree,
    PackManifest,
} from "@/lib/types";
import {
    baseName,
    holdsRootFile,
    inRange,
    ROOT_EULA,
    ROOT_PROPERTIES,
    SERVER_PORT_RANGE,
} from "@/lib/types";
import { formatSize } from "@/lib/format";
import { RISE } from "@/lib/springs";
import {
    Btn,
    Collapse,
    Divider,
    InlineRow,
    NoteRow,
    Panel,
    PanelHead,
    SearchSelect,
    Stepper,
    Swap,
    TextInput,
    Tip,
    TIP_TRIGGER,
    Toggle,
    HOVER_FILL,
    type SelectOption,
} from "@/components/ui";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";
import { difficultyOptions, findDirNode, findFileNode, gamemodeOptions } from "./constants";

/** 单包参数覆写入口（来自 ConvertPage 的 options state） */
type Patch = (p: Partial<ConversionOptions>) => void;

/* ---------------- 运行环境：三个版本下拉 + 启动脚本开关 ---------------- */

/* 各卡的 `readOnly` = 回看态：控件一律灰化成「一眼看出点不动」的样子
   （下拉与输入框压成凹底、开关与勾选框整件压一档、步进器只留值区），卡片不换排版——
   回看要核对的就是「当时那套配置长什么样」，所以压的是容器，数值与颜色保持读得清。 */

/** 「自动选择」在下拉里的 value。它不是路径 ⇒ 落盘时压成空串，后端按"没指定"走自动挑 */
export const JAVA_AUTO = "auto";

/**
 * 本机候选打上显示名：只有一枚 17 就叫 `Java 17`，两枚以上才编号 `Java 17(1)/(2)`——
 * 单 JDK 机器上那个 `(1)` 是没来由的噪音。
 *
 * 序号是**渲染期算出来的显示别名**，不进快照：顺序跟着 `JAVA_HOME` → PATH 走，装或卸一枚就整体重排，
 * 拿它当标识会指错 JDK。真正的标识只有路径，所以序号会变、选中的那枚不会。
 */
export function labelInstalls(installed: JavaInstall[]) {
    const total = new Map<number, number>();
    for (const j of installed) total.set(j.major, (total.get(j.major) ?? 0) + 1);
    const seen = new Map<number, number>();
    return installed.map((j) => {
        const nth = (seen.get(j.major) ?? 0) + 1;
        seen.set(j.major, nth);
        return { ...j, label: `Java ${j.major}${(total.get(j.major) ?? 1) > 1 ? `(${nth})` : ""}` };
    });
}

export function RuntimeEnvCard({
    options,
    patch,
    manifest,
    loader,
    mcOptions,
    loaderOptions,
    javaProbe,
    onJavaProbe,
    readOnly,
}: {
    options: ConversionOptions | null;
    patch: Patch;
    manifest: PackManifest;
    /** 加载器显示名（loaderLabel 结果，用于 Loader 下拉的标签） */
    loader: string;
    mcOptions: SelectOption[];
    loaderOptions: SelectOption[];
    /** 本机 JDK 探测结果（只在开了「本机安装 Loader」时才有值；null = 还在探）。
     *  它同时是那颗下拉的候选来源：事前检查与候选必须是同一份事实，不然提示说的和列表给的对不上。
     *  回看态（`readOnly`）不传：那一态要核对的是「当时那套配置」，本机现在有没有 Java 与它无关。 */
    javaProbe?: JavaProbe | null;
    /** 点开 Java 下拉时重探一次：候选是本机实探出来的，用户中途装了 JDK 不会自己触发重探 */
    onJavaProbe?: () => void;
    readOnly?: boolean;
}) {
    const t = useT();
    const installLoader = options?.installLoaderLocally ?? true;
    // Fabric 没有 installer 可跑：它的 loader jar 与 launcher 直接从版本表取，这一档对本包是空开关
    // （实测：meta 给的 server/jar 是「首启自装」的启动器，不是一份装好的树）。
    const needsInstaller = manifest.loader !== "fabric";
    /**
     * 这一档此刻有没有读者：只有"真的会在本机跑 installer"那一格分支有。
     * Fabric、关掉开关、回看态三处统一灰化（灰化而不是藏掉：这一格在报告和回看里都要露面，
     * 值得看得见，只是点不动）。
     */
    const javaLive = needsInstaller && installLoader && !readOnly;
    const installs = labelInstalls(javaProbe?.installed ?? []);
    /** 手选那枚已经不在本机了（卸载/换机）⇒ 界面如实显示「自动选择」，因为正在用的就是它 */
    const selectedMissing = !!javaProbe?.selectedMissing;
    /**
     * 本机一枚 JDK 都没扫到。这一态和"有 Java 但不够格"是两句不同的话：
     * 后者答得出"本次用哪一枚"（那枚就在列表里，只是低了），前者连问题都不成立 ⇒
     * 「本次使用 自动选择」会指着一个不存在的东西，必须换成"这里压根没有"。
     */
    const noJava = javaProbe?.installed.length === 0;
    /**
     * 点不动的那一态（Fabric、关了开关、回看）一律显示「自动选择」：这一档没有读者，
     * 摆一个需求线数字上去看着像"这次用的是 17"，而那串其实是包的要求、不是这台机器的答案。
     * 需求线该露面的地方是那行提示与报告，不是一颗灰掉的下拉。
     */
    const javaValue =
        !javaLive || !options?.javaPath || selectedMissing
            ? JAVA_AUTO
            : options.javaPath;
    /** note 里那个带颜色的词：自动态说"自动选择"，手选态说那枚的显示名（序号现算，不进快照） */
    const pickedWord =
        javaValue === JAVA_AUTO
            ? t("convert.auto-select", "自动选择")
            : (installs.find((j) => j.path === javaValue)?.label ?? t("convert.auto-select", "自动选择"));
    const required = javaProbe?.requiredMajor ?? null;

    return (
        <Panel gap={14}>
            <PanelHead title={t("convert.runtime", "运行环境")} />
            <div className="flex w-full gap-3">
                <SearchSelect
                    className="flex-1"
                    label={t("convert.minecraft-version", "Minecraft 版本")}
                    value={options?.mcVersion ?? manifest.mcVersion}
                    options={mcOptions}
                    // 裸 zip 认不出版本时后端给空串（不再凭空造一档）：这一格显示「未识别」，
                    // 选没选由下面那行提示和「开始转换」那道闸说，不在框里堆第二句
                    placeholder={t("convert.unrecognized", "未识别")}
                    searchable
                    searchPlaceholder={t("convert.search-versions", "搜索版本…")}
                    readOnly={readOnly}
                    onChange={(v) => patch({ mcVersion: v })}
                />
                <SearchSelect
                    className="flex-1"
                    label={`${loader} Loader`}
                    value={options?.loaderVersion ?? ""}
                    options={loaderOptions}
                    // 裸 zip 没有 dependencies 段：列表到位前这一格是空的，给一句「待选择」而不是留白
                    placeholder={t("convert.await-loader-version", "待选择")}
                    searchable
                    searchPlaceholder={t("convert.search-versions", "搜索版本…")}
                    readOnly={readOnly}
                    onChange={(v) => patch({ loaderVersion: v })}
                />
                <SearchSelect
                    className="flex-1"
                    // 标签说"本机 Java"而不是"Java 版本"：这一档列的是这台机器上装了的那几枚，
                    // 包要什么那一档是需求线，写在下面那行提示里
                    label={t("convert.local-java", "本地 Java 环境")}
                    value={javaValue}
                    options={
                        javaLive
                            ? [
                                  { value: JAVA_AUTO, label: t("convert.auto-select", "自动选择") },
                                  ...installs.map((j) => ({ value: j.path, label: j.label })),
                              ]
                            : // 灰化那一态既不探测也没得选，只把「自动选择」这一项摆着
                              [{ value: JAVA_AUTO, label: t("convert.auto-select", "自动选择") }]
                    }
                    readOnly={!javaLive}
                    onOpen={onJavaProbe}
                    onChange={(v) => patch({ javaPath: v === JAVA_AUTO ? "" : v })}
                />
            </div>
            <Divider />
            <InlineRow label={t("convert.generate-start", "生成启动脚本（start.sh / start.bat）")}>
                <Toggle
                    checked={options?.generateScripts ?? true}
                    readOnly={readOnly}
                    onChange={(v) => patch({ generateScripts: v })}
                />
            </InlineRow>
            {/* 两格都走 Collapse：关了开关时提示行是当场卸载的，卡高硬跳、下面五张卡同帧重排
                （Panel 是 gap=14 的 flex 列 ⇒ gap={14}，理由见 @/components/ui/Collapse） */}
            <Collapse when={needsInstaller} gap={14}>
                <InlineRow label={t("convert.install-loader", "本机安装 Loader（产物上传即开服）")}>
                    <Toggle
                        checked={installLoader}
                        readOnly={readOnly}
                        onChange={(v) => patch({ installLoaderLocally: v })}
                    />
                </InlineRow>
            </Collapse>
            <Collapse when={javaLive} gap={14}>
                <NoteRow
                    icon={javaProbe?.status === "fail" ? AlertTriangle : Info}
                    tone={javaProbe?.status === "fail" ? "danger" : undefined}
                >
                    {javaProbe ? (
                        noJava ? (
                            <>
                                {t("convert.java-detected", "本机没有检测到 Java")}
                                {required
                                    ? t("convert.requires-java", "（本次需要 Java {{major}} 及以上）", { major: required })
                                    : ""}
                            </>
                        ) : (
                            <>
                                {t("convert.using", "本次使用")}{" "}
                                <span
                                    className={cn(
                                        TIP_TRIGGER,
                                        // 走色跟整行一致（红态压一枚蓝上去，读起来是两处不同的坏消息），
                                        // 可悬的凭据换成同色的一层薄纱实线：这个字号的点线读着像碎屑，满色实线又太重
                                        javaProbe?.status === "fail"
                                            ? "text-redstone decoration-redstone/40"
                                            : "text-accent decoration-accent/40",
                                        "underline decoration-1 underline-offset-2"
                                    )}
                                    // 气泡是纯 hover 的，读屏与键盘要有一条同等信息的出口
                                    aria-label={
                                        javaProbe.javaPath ?? t("convert.usable-java", "本机没有可用的 Java")
                                    }
                                >
                                    {pickedWord}
                                    {/* 手选那枚靠 `(1)/(2)` 分得开，但"到底用的哪一份"只有路径答得出：
                                        悬浮给全路径，长路径交给 wide 那档等宽断行 */}
                                    <Tip
                                        label={javaProbe.javaPath ?? t("convert.usable-java", "本机没有可用的 Java")}
                                        wide
                                        align="start"
                                        side="top"
                                    />
                                </span>
                                {required
                                    ? t("convert.requires-java", "（本次需要 Java {{major}} 及以上）", { major: required })
                                    : ""}
                                {selectedMissing
                                    ? t("convert.jdk-picked", " · 方案里指定的那枚已不在本机")
                                    : ""}
                            </>
                        )
                    ) : (
                        t("convert.detecting-local", "正在检测本机 Java…")
                    )}
                </NoteRow>
            </Collapse>
        </Panel>
    );
}

/* ---------------- 客户端保留内容：默认空态，「添加内容」弹窗主动勾选 ---------------- */

export function KeepDirsCard({
    options,
    packTree,
    parsed = true,
    onPick,
    onRemove,
    onRemoveFile,
    readOnly,
}: {
    options: ConversionOptions | null;
    /** 勾选弹窗数据源 + 行内读数（目录带递归文件数与体积，根级文件带自身大小） */
    packTree: PackDirTree;
    /** 源包是否还能解析出目录树：回看旧任务时源文件可能已被移走，那时不能谎称「包内没有资源」 */
    parsed?: boolean;
    /** 只读视图（任务详情「方案」签）不传写入口：静态渲染下没有触发路径 */
    onPick?: () => void;
    /** 卡片行内移除单个保留目录（根级文件走 onRemoveFile；批量增删走 DirPickerModal 应用回写） */
    onRemove?: (path: string) => void;
    /** 行内移除单个根级保留文件 */
    onRemoveFile?: (path: string) => void;
    readOnly?: boolean;
}) {
    const keepDirs = options?.keepDirs ?? [];
    const keepFiles = options?.keepFiles ?? [];
    const t = useT();
    const isEmpty = keepDirs.length === 0 && keepFiles.length === 0;
    const nothingToPick = packTree.dirs.length === 0 && packTree.files.length === 0;

    /** 清单里的一行（目录与根级文件同一条版）：图标 + 名字 + 右侧读数 + 移除键。
     *  建在组件体内、由两类条目各调一次：`AnimatePresence mode="popLayout"` 要直接拿到运动节点，
     *  外面再套一层自定义组件它就量不到 DOM；放在这里也顺带让 `t` 只调一次（勾满几十条时不至于反复取句柄） */
    const row = (
        key: string,
        name: string,
        Icon: LucideIcon,
        reading: ReactNode,
        readingMuted: boolean,
        onRemove?: () => void
    ) => (
        <motion.div
            key={key}
            layout
            initial={{ opacity: 0, y: -6 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -6 }}
            transition={RISE}
            className={`flex min-w-0 items-center gap-2.5 rounded-lg px-1.5 py-1 hover:bg-surface-2 ${HOVER_FILL}`}
        >
            <Icon className="size-3.5 shrink-0 text-accent" />
            <span className="min-w-0 flex-1 truncate font-mono text-[12px] leading-[18px] font-medium text-text-1">
                {name}
            </span>
            {reading !== null && (
                <span
                    className={cn(
                        "shrink-0 font-mono text-[11px] leading-[16px] tabular-nums",
                        readingMuted ? "text-text-3" : "text-emerald"
                    )}
                >
                    {reading}
                </span>
            )}
            {onRemove && (
                <button
                    onClick={onRemove}
                    aria-label={t("convert.remove", "移除")}
                    className={cn(
                        TIP_TRIGGER,
                        "size-6 rounded-md text-text-3",
                        "flex shrink-0 items-center justify-center",
                        "hover:bg-redstone-dim hover:text-redstone",
                        HOVER_FILL
                    )}
                >
                    <X className="size-3" />
                    <Tip label={t("convert.remove", "移除")} />
                </button>
            )}
        </motion.div>
    );

    return (
        <Panel gap={14}>
            <PanelHead
                title={t("convert.kept-client", "客户端保留内容")}
                right={
                    readOnly ? undefined : (
                        <Btn
                            size="sm"
                            icon={Plus}
                            disabled={nothingToPick}
                            onClick={() => onPick?.()}
                        >
                            {t("convert.add-folder", "添加内容")}
                        </Btn>
                    )
                }
            />
            {/* 空态↔清单是同位换批（不是长出来），走 Swap：旧层当场让位、新层立刻占位，
                两层的进出场节拍由 Swap 自己带 */}
            <Swap swapKey={isEmpty ? "empty" : "list"}>
                {isEmpty ? (
                    <p className="w-full py-3 text-center text-[11px] text-text-3">
                        {readOnly
                            ? t("convert.task-kept", "该任务未保留任何包内目录与文件")
                            : nothingToPick
                              ? parsed
                                    ? t("convert.keepable-folders", "包内未检测到可保留的内容（mods 之外没有资源文件）")
                                    : t("convert.source-pack-missing", "源包已不在原位置 · 该任务未保留任何包内目录与文件")
                              : t("convert.folder-selected", "尚未选择 · 点击上方「添加内容」从包内勾选")}
                    </p>
                ) : (
                    <div className="flex w-full flex-col gap-1">
                        <AnimatePresence initial={false} mode="popLayout">
                            {keepDirs.map((p) => {
                                const dir = findDirNode(packTree.dirs, p);
                                return row(
                                    p,
                                    `${p}/`,
                                    Folder,
                                    // 源包不在原位置时读不到聚合数：宁缺勿假
                                    dir ? (
                                        <>
                                            {t("convert.count-file", "{{count}} 文件", { count: dir.fileCount })}
                                            {" · "}
                                            {formatSize(dir.sizeBytes)}
                                        </>
                                    ) : null,
                                    false,
                                    readOnly ? undefined : () => onRemove?.(p)
                                );
                            })}
                            {keepFiles.map((p) => {
                                const f = findFileNode(packTree, p);
                                return row(
                                    p,
                                    f?.name ?? baseName(p),
                                    FileText,
                                    // 源包不在时读不到大小：留一行灰字而不是编一个 0 B
                                    f ? formatSize(f.sizeBytes) : t("convert.size-unknown", "大小未知"),
                                    !f,
                                    readOnly ? undefined : () => onRemoveFile?.(p)
                                );
                            })}
                        </AnimatePresence>
                    </div>
                )}
            </Swap>
            <NoteRow icon={Info}>
                {readOnly
                    ? t("convert.folders-copied", "这些目录与文件当时从源包带入，各自按自己的名字落在产物根目录")
                    : parsed
                      ? t("convert.selected-folders", "勾哪一层就落哪一层：目录整棵（内部层级保留）、文件单个，都挂在产物根目录下")
                      : t("convert.source-pack", "源包已不在原位置 · 无法浏览包内内容，已有条目仍可移除")}
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
    const t = useT();
    // 勾了包内那份 eula.txt ⇒ builder 让位（判据单源在后端 emit_root），这个开关这次不进产物。
    // 行高按设计稿钉死，所以这里只把标签当场改口、并把开关灰化，不另起一行说明
    const eulaYielded = holdsRootFile(options?.keepFiles, ROOT_EULA);
    return (
        <Panel gap={14}>
            <PanelHead title={t("convert.launch-args", "启动参数")} />
            <InlineRow label={t("convert.max-server", "服务器内存上限")}>
                <Stepper
                    value={Math.round((options?.memoryMb ?? 6144) / 1024)}
                    min={1}
                    max={32}
                    suffix="GB"
                    readOnly={readOnly}
                    onChange={(v) => patch({ memoryMb: v * 1024 })}
                />
            </InlineRow>
            <InlineRow label={t("convert.start-headless", "无界面模式启动（--nogui）")}>
                <Toggle
                    checked={options?.nogui ?? false}
                    readOnly={readOnly}
                    onChange={(v) => patch({ nogui: v })}
                />
            </InlineRow>
            <InlineRow
                label={
                    eulaYielded
                        ? t("convert.write-eula-kept", "eula.txt 取自包内 · 本项不进产物")
                        : t("convert.write-eula", "自动写入 eula=true（同意 Mojang EULA）")
                }
            >
                <Toggle
                    checked={options?.agreeEula ?? true}
                    readOnly={readOnly || eulaYielded}
                    onChange={(v) => patch({ agreeEula: v })}
                />
            </InlineRow>
            <InlineRow label={t("convert.aikar-flags", "Aikar's flags 优化参数组（G1GC 推荐）")}>
                <Toggle
                    checked={options?.useAikarFlags ?? true}
                    readOnly={readOnly}
                    onChange={(v) => patch({ useAikarFlags: v })}
                />
            </InlineRow>
            <InlineRow label={t("convert.extra-jvm", "附加 JVM 参数")}>
                <TextInput
                    className="w-[240px]"
                    value={options?.extraJvmArgs ?? ""}
                    readOnly={readOnly}
                    onChange={(e) => patch({ extraJvmArgs: e.target.value })}
                    placeholder={t("convert.appended-start", "原样拼入 start 脚本")}
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
    const t = useT();
    // 勾了包内那份 server.properties ⇒ builder 整份沿用（不让位就会丢掉包内那些界面没暴露的键），
    // 这 8 项这次不进产物 ⇒ 控件按只读口径灰化，底栏说明当场改口
    const propsYielded = holdsRootFile(options?.keepFiles, ROOT_PROPERTIES);
    const ro = readOnly || propsYielded;
    return (
        <Panel gap={14}>
            <PanelHead title={t("convert.server-settings", "服务端设置")} />
            <div className="flex w-full gap-3">
                <SearchSelect
                    className="flex-1"
                    label={t("convert.game-mode", "游戏模式")}
                    value={options?.gamemode ?? "survival"}
                    options={gamemodeOptions()}
                    readOnly={ro}
                    onChange={(v) => patch({ gamemode: v as ConversionOptions["gamemode"] })}
                />
                <SearchSelect
                    className="flex-1"
                    label={t("convert.difficulty", "难度")}
                    value={options?.difficulty ?? "easy"}
                    options={difficultyOptions()}
                    readOnly={ro}
                    onChange={(v) => patch({ difficulty: v as ConversionOptions["difficulty"] })}
                />
            </div>
            <div className="grid w-full grid-cols-2 gap-3">
                <Field label={t("convert.server-port", "服务器端口")}>
                    <NumField
                        value={options?.serverPort ?? 25565}
                        min={SERVER_PORT_RANGE.min}
                        max={SERVER_PORT_RANGE.max}
                        clamp={false}
                        readOnly={ro}
                        onCommit={(v) => patch({ serverPort: v })}
                    />
                </Field>
                <Field label={t("convert.max-players", "最大人数")}>
                    <NumField
                        value={options?.maxPlayers ?? 20}
                        min={1}
                        max={1000}
                        readOnly={ro}
                        onCommit={(v) => patch({ maxPlayers: v })}
                    />
                </Field>
            </div>
            <Field label={t("convert.server-description", "服务器描述（MOTD）")}>
                <TextInput
                    className="w-full"
                    value={options?.motd ?? ""}
                    readOnly={ro}
                    onChange={(e) => patch({ motd: e.target.value })}
                    placeholder={t("convert.shown-server", "显示在服务器列表中的一行描述")}
                    spellCheck={false}
                />
            </Field>
            <Field label={t("convert.world-seed", "世界种子（留空 = 随机生成）")}>
                <TextInput
                    className="w-full"
                    value={options?.levelSeed ?? ""}
                    readOnly={ro}
                    onChange={(e) => patch({ levelSeed: e.target.value })}
                    placeholder={t("convert.4045151867437057206", "如 4045151867437057206")}
                    spellCheck={false}
                />
            </Field>
            <Divider />
            <InlineRow label={t("convert.verified-accounts", "正版验证（online-mode）")}>
                <Toggle
                    checked={options?.onlineMode ?? true}
                    readOnly={ro}
                    onChange={(v) => patch({ onlineMode: v })}
                />
            </InlineRow>
            <NoteRow icon={Info}>
                {propsYielded
                    ? t("convert.server-fields-kept", "已勾包内 server.properties · 以上字段不进产物，属性以那份为准")
                    : t("convert.writes-server", "以上字段写入包内 server.properties · 勾了包内同名文件时以那份为准")}
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

/**
 * 数字输入：本地草稿允许瞬时空串/半成品。
 * 默认失焦时 clamp 提交（越界自己吸回边界，界面无感）。`clamp={false}` 那格改成**逐字实时提交、不吸边**：
 * 越界的数照实存进方案，红字与「开始转换」的禁用读的都是方案里那个数（一个真源，不会出现染红却不拦、
 * 或拦了却看不出错在哪）。空串按 0 提交 ⇒ 0 落在任何 `min ≥ 1` 的区间外，"还没填"与"填错"共用一条出口。
 */
function NumField({
    value,
    min,
    max,
    readOnly,
    onCommit,
    clamp = true,
}: {
    value: number;
    min: number;
    max: number;
    readOnly?: boolean;
    onCommit: (v: number) => void;
    clamp?: boolean;
}) {
    const [draft, setDraft] = useState(String(value));
    // 只在外部换值时同步草稿（默认值到达 / 清空我的修改 / 回看）；自己打字绕回来的一圈别把 "07" 改成 "7"
    useEffect(() => setDraft((d) => (parseInt(d, 10) === value ? d : String(value))), [value]);
    const digits = (raw: string) => {
        const n = parseInt(raw, 10);
        return Number.isFinite(n) ? n : null;
    };
    return (
        <TextInput
            className="w-full"
            inputMode="numeric"
            value={draft}
            readOnly={readOnly}
            invalid={!clamp && !inRange(value, { min, max })}
            onChange={(e) => {
                const d = e.target.value.replace(/\D/g, "").slice(0, 6);
                setDraft(d);
                if (!clamp) onCommit(digits(d) ?? 0);
            }}
            onBlur={() => {
                const n = digits(draft);
                // 不吸边那格：空串也已经有 0 存着了（onChange 实时提交过），这里只把草稿对齐回去
                const c = clamp ? (n === null ? value : Math.min(max, Math.max(min, n))) : (n ?? 0);
                setDraft(String(c));
                onCommit(c);
            }}
        />
    );
}
