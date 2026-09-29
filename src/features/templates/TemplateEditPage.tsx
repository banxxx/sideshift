/**
 * 模板编辑页（二级页 · 设计稿 `.scratch/templates-page-proto.html` 屏 ②（编辑）/ ⑤（新建）+ 稿外状态件 ①②③）
 *
 * 这一页只回答一个问题：**这份模板要带走哪几档**。所以每行的勾才是主角，控件是从属——
 * 未勾的行一律按只读口径灰化（`readOnly` 而不是「看得见点不动」的假出口），勾上才恢复可改。
 * 稿内屏 ⑤ 把开关与步进器画成活的，这里没有照抄：那一屏所有行都没勾，活的控件读起来就是
 * 「这里能改」，而改了也不会进模板，属于误导。
 *
 * **两态共用一套排版，差别只在数据**：
 *  - 编辑：`values` 取模板存的那几档 ⇒ 未勾的行没有值可显示，那一格如实写「未纳入」。
 *  - 新建：`values` 整份带着种子（转换页当时的值，或 `template_defaults` 的默认值），一档都没勾
 *    ⇒ 未勾的行显示那个**灰着的现值**，它回答的是「现在勾上会带走什么」。
 *  所以「勾没勾」只由 `ticked` 决定，`values[k] !== undefined` 只管那一格有没有东西可显示。
 *  保存交回 `pickTemplateValues`：勾中的键才写盘，屏幕上的其余值一个都不带。
 *
 * 未勾那几档**不是**「按默认值来」：套用时它们压根不被触碰，转换页保持原值。页头那句话说的就是这件事。
 *
 * 保存只有一条路：整张表交回后端（`use-template-table` 的 `commit`，与列表页同一份口径 ⇒
 * 「保存失败」那句提示不会在两页长成两种样子）。落盘成功才 `back()`，失败留在这一页、草稿还在。
 * 脏判定只对**会落盘的那三样**求指纹（名称 / 备注 / 勾中的值）：改一个没勾的框不该拦住返回。
 * 未保存时点返回不弹窗——照模组方案「确认清空」那同款两段式，页头那枚「未保存」原位换成
 * 两枚就地确认（稿外件 ②），弹窗在这里只是多一层要关的东西。
 *
 * 启动参数那张卡的计数芯片会染金：`generateScripts` 在模板里且为 false 时，内存 / nogui /
 * Aikar / 附加 JVM 这四档在产物里根本没有读者（`core/builder.rs` 里它们全在 `if generate_scripts`
 * 块内，eula 与 server.properties 不在）。一个字都不多写，颜色就是那一档判定。
 */
import { Folder, Save } from "lucide-react";
import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { motion } from "motion/react";
import * as api from "@/lib/api";
import { notify } from "@/lib/notify";
import { useT } from "@/lib/i18n";
import { useNavigation } from "@/lib/navigation";
import { CARD_RISE, PAGE_RISE } from "@/lib/page-motion";
import { truncateMiddle } from "@/lib/format";
import {
    SERVER_PORT_RANGE,
    TEMPLATE_FIELD_COUNT,
    TEMPLATE_FIELD_KEYS,
    TEMPLATE_NAME_MAX,
    TEMPLATE_NOTE_MAX,
    pickTemplateValues,
    templateFields,
    templateValueCount,
    uniqueTemplateName,
    type ConversionTemplate,
    type JavaProbe,
    type TemplateFieldKey,
    type TemplateFieldMeta,
    type TemplateGroup,
    type TemplateValues,
} from "@/lib/types";
import {
    Btn,
    CheckBox,
    CountRow,
    Divider,
    LinkBtn,
    PageHeader,
    Panel,
    PanelHead,
    READONLY_BOX,
    SearchSelect,
    SectionTitle,
    Stepper,
    TextInput,
    Toggle,
    ToneChip,
    type SelectOption,
} from "@/components/ui";
import { cn } from "@/lib/utils";
import { difficultyOptions, gamemodeOptions } from "../convert/constants";
import { JAVA_AUTO, labelInstalls } from "../convert/OptionCards";
import { DeleteTemplateModal } from "./DeleteTemplateModal";
import { guardTemplateCap, useTemplateTable } from "./use-template-table";

/** 编辑器草稿：`values` 是屏上那些值（含未勾的），`ticked` 才是「进不进模板」的唯一判据 */
interface EditDraft {
    name: string;
    note: string;
    values: TemplateValues;
    ticked: Set<TemplateFieldKey>;
}

/**
 * 关掉「生成启动脚本」后在产物里没有读者的四档（<code>builder.rs</code> 的 `if generate_scripts` 段）。
 * EULA 不在其中：`eula.txt` 恒生成，那档只决定里面的值。
 */
const SCRIPT_ONLY_KEYS: TemplateFieldKey[] = ["memoryMb", "nogui", "useAikarFlags", "extraJvmArgs"];

/** 脏判定用的指纹：数组顺序按 `TEMPLATE_FIELD_KEYS` 定死，同一个草稿永远得到同一个串 */
const sigOf = (name: string, note: string, values: TemplateValues) =>
    JSON.stringify([name.trim(), note.trim(), TEMPLATE_FIELD_KEYS.map((k) => values[k] ?? null)]);

/**
 * 逐键写值。`TemplateValues` 的键是 16 个字段名的联合，`values[k] = v` 会被 TS 判成
 * 「要同时满足所有属性类型」而拒收 ⇒ 写值统一走这一处，调用方不必各自 `as`。
 */
const withValue = (values: TemplateValues, key: TemplateFieldKey, value: unknown): TemplateValues => ({
    ...values,
    [key]: value,
});

export function TemplateEditPage() {
    const t = useT();
    const { entry, back, setLeaveGuard } = useNavigation();
    const { templates, loaded, commit } = useTemplateTable();

    const templateId = entry.params?.templateId as string | undefined;
    /** 「存为模板…」带上来的种子：16 档全在（转换页当时那套值），一档都没勾 */
    const seed = entry.params?.seed as TemplateValues | undefined;
    const isNew = templateId === undefined;
    const target = templateId ? templates.find((x) => x.id === templateId) : undefined;

    const [draft, setDraft] = useState<EditDraft | null>(null);
    /** 默认值表（Rust: `template_defaults`）：新建态的整份种子 + 编辑态勾上一行空档时的起填值 */
    const [fb, setFb] = useState<TemplateValues | null>(null);
    /** 这一页的「干净」基准：进页面时那一份，保存成功后换成刚落盘的那一份 */
    const [baseSig, setBaseSig] = useState<string | null>(null);
    const [confirmingLeave, setConfirmingLeave] = useState(false);
    const [pendingDelete, setPendingDelete] = useState(false);
    const [javaProbe, setJavaProbe] = useState<JavaProbe | null>(null);

    useEffect(() => {
        let dead = false;
        api
            .templateDefaults()
            .then((d) => !dead && setFb(d))
            // 读不到也得放行：闸门压着会让这一页永远停在占位上，比少了那份起填值更糟
            .catch(() => !dead && setFb({}));
        return () => {
            dead = true;
        };
    }, []);

    // 草稿只铺一次（`draft` 非空即返回）：模板表还要异步读，读数到手前不能先拿空草稿占住这一页
    useEffect(() => {
        if (draft || !loaded || fb === null) return;
        if (target) {
            setDraft({
                name: target.name,
                note: target.note,
                values: { ...target.values },
                ticked: new Set(TEMPLATE_FIELD_KEYS.filter((k) => target.values[k] !== undefined)),
            });
            setBaseSig(sigOf(target.name, target.note, target.values));
        } else if (isNew) {
            setDraft({ name: "", note: "", values: seed ?? fb, ticked: new Set() });
            setBaseSig(sigOf("", "", {}));
        }
    }, [draft, loaded, fb, target, isNew, seed]);

    const isIn = (k: TemplateFieldKey) => draft?.ticked.has(k) ?? false;
    const picked = draft ? pickTemplateValues(draft.values, draft.ticked) : {};
    const count = templateValueCount(picked);
    const trimmed = (draft?.name ?? "").trim();
    /** 同名拦截：名称是这一族界面认人的凭据（列表两行同名、下拉两项同读数都读不出是谁） */
    const dupe =
        trimmed.length > 0 && templates.some((x) => x.name === trimmed && x.id !== templateId);
    const dirty = draft !== null && baseSig !== null && sigOf(draft.name, draft.note, picked) !== baseSig;
    /** 结构信号：模板会关掉启动脚本，却还带着那四档只有脚本能读到的值 */
    const scriptsDead =
        picked.generateScripts === false && SCRIPT_ONLY_KEYS.some((k) => picked[k] !== undefined);
    const canSave = draft !== null && trimmed.length > 0 && !dupe && count > 0;

    // Java 候选来自本机实探（与转换页同一颗 `probe_java`）。需求线传 null：
    // 模板不绑 MC 版本，这一档没有「够不够」可判，只要「本机有什么」。
    const javaPathValue = String(draft?.values.javaPath ?? "");
    const javaLive = isIn("javaPath");
    const probeJava = useCallback(() => {
        void api.probeJava(null, javaPathValue).then(setJavaProbe);
    }, [javaPathValue]);
    useEffect(() => {
        if (javaLive) probeJava();
    }, [javaLive, probeJava]);

    // 未保存拦截：返回与切一级页都过这一道（口径见 navigation.tsx 的 LeaveGuard）
    const dirtyRef = useRef(false);
    const confirmingRef = useRef(false);
    const proceedRef = useRef<(() => void) | null>(null);
    useEffect(() => {
        dirtyRef.current = dirty;
    }, [dirty]);
    useEffect(() => {
        confirmingRef.current = confirmingLeave;
    }, [confirmingLeave]);
    useEffect(() => {
        setLeaveGuard((proceed) => {
            if (!dirtyRef.current) return false;
            // 已经在问了 ⇒ 再点返回还是不走：两枚就地确认才是出口
            if (!confirmingRef.current) {
                proceedRef.current = proceed;
                setConfirmingLeave(true);
            }
            return true;
        });
        return () => setLeaveGuard(null);
    }, [setLeaveGuard]);

    /** 放弃修改：先把脏标记按下去再放行（`proceed` 是从拦截器手里接过来的那趟导航） */
    const discard = () => {
        dirtyRef.current = false;
        const proceed = proceedRef.current;
        proceedRef.current = null;
        setConfirmingLeave(false);
        if (proceed) proceed();
        else back();
    };

    /**
     * 页头只在这里建一次，占位态与草稿态共用同一棵壳。分成两棵写会当场 remount：
     * 模板表读数到手那一刻整页重挂，`PAGE_RISE`/`CARD_RISE` 再演一遍 ⇒ 他报的「左上角文字闪一下」。
     * `sub` 同理得在占位态就给（`isNew` 来自路由参数，不需要草稿），否则标题还会先矮后高跳一格。
     */
    const header = (
        <motion.div variants={CARD_RISE}>
            <PageHeader
                title={
                    isNew
                        ? t("templates.new-template", "新建模板")
                        : t("templates.edit-title", "编辑模板")
                }
                sub={
                    isNew
                        ? t("templates.new-sub", "值取自当前转换页 · 勾哪行才写进模板")
                        : t("templates.edit-sub", "勾中的字段才会写入 · 未勾的保持当前值")
                }
                right={
                    !draft ? undefined : confirmingLeave ? (
                        <div className="flex shrink-0 items-center gap-3.5">
                            <LinkBtn size="sm" onClick={discard}>
                                {t("templates.discard", "放弃修改")}
                            </LinkBtn>
                            <LinkBtn
                                size="sm"
                                className="text-text-3 hover:text-text-1"
                                onClick={() => setConfirmingLeave(false)}
                            >
                                {t("templates.keep-editing", "继续编辑")}
                            </LinkBtn>
                        </div>
                    ) : isNew && !dirty ? undefined : (
                        <ToneChip tone={dirty ? "gold" : "muted"}>
                            {dirty ? t("templates.unsaved", "未保存") : t("templates.saved", "已保存")}
                        </ToneChip>
                    )
                }
            />
        </motion.div>
    );

    if (!draft) {
        // 读盘没到手 = 占位；到手了却没有这一份模板 = 如实说找不到（在别的页被删掉那一档）
        const missing = !isNew && loaded && fb !== null && !target;
        return (
            <motion.div
                className="flex flex-col gap-5 overflow-hidden py-6"
                variants={PAGE_RISE}
                initial="hidden"
                animate="show"
            >
                {header}
                <motion.div variants={CARD_RISE}>
                    <Panel className="items-center py-16">
                        <p className="text-[13px] text-text-2">
                            {missing
                                ? t("templates.missing", "模板不存在或已被删除。")
                                : t("templates.reading", "正在读取模板…")}
                        </p>
                        {missing && (
                            <Btn variant="primary" size="sm" className="mt-1" onClick={() => back()}>
                                {t("templates.back-list", "返回模板列表")}
                            </Btn>
                        )}
                    </Panel>
                </motion.div>
            </motion.div>
        );
    }

    const { name, note, values } = draft;
    const metas = templateFields();
    const byKey = Object.fromEntries(metas.map((m) => [m.key, m])) as Record<
        TemplateFieldKey,
        TemplateFieldMeta
    >;
    const groupOf = (g: TemplateGroup) => metas.filter((m) => m.group === g);

    const patchDraft = (p: Partial<EditDraft>) => setDraft((d) => (d ? { ...d, ...p } : d));
    const setOne = (k: TemplateFieldKey, v: unknown) =>
        setDraft((d) => (d ? { ...d, values: withValue(d.values, k, v) } : d));

    /** 勾/取消：勾上一行空档（编辑态那些「未纳入」）时先填默认值，不给它留一个空框 */
    const tick = (k: TemplateFieldKey, on: boolean) =>
        setDraft((d) => {
            if (!d) return d;
            const ticked = new Set(d.ticked);
            if (!on) {
                ticked.delete(k);
                return { ...d, ticked };
            }
            ticked.add(k);
            return { ...d, ticked, values: withValue(d.values, k, d.values[k] ?? fb?.[k]) };
        });

    const javaOptions: SelectOption[] = [
        { value: JAVA_AUTO, label: t("convert.auto-select", "自动选择") },
        ...labelInstalls(javaProbe?.installed ?? []).map((j) => ({ value: j.path, label: j.label })),
    ];

    /** 未纳入（编辑态那一格）与这一档自己的提示语：前者只在没有值可显示时出场 */
    const notIn = t("templates.not-included", "未纳入");
    const textPh = (k: TemplateFieldKey) =>
        k === "extraJvmArgs"
            ? t("convert.appended-start", "原样拼入 start 脚本")
            : k === "motd"
              ? t("convert.shown-server", "显示在服务器列表中的一行描述")
              : t("convert.4045151867437057206", "如 4045151867437057206");

    const selectOptions = (k: TemplateFieldKey): SelectOption[] =>
        k === "javaPath"
            ? javaOptions
            : k === "gamemode"
              ? gamemodeOptions()
              : difficultyOptions();

    /**
     * 一行右端（或下方）那枚控件。`stacked` 决定宽度（整宽 vs 稿内那 200px）与要不要自带小标；
     * `readOnly` 一律问 `ticked`，而不是问有没有值——灰化说的是「这档不进模板」。
     */
    const controlOf = (m: TemplateFieldMeta, stacked = false): ReactNode => {
        const k = m.key;
        const raw = values[k];
        const present = raw !== undefined;
        const on = isIn(k);
        const w = stacked ? "w-full" : "w-[200px]";
        switch (m.kind) {
            case "toggle":
                return <Toggle checked={Boolean(raw)} readOnly={!on} onChange={(v) => setOne(k, v)} />;
            case "stepper":
                // 存 MB、显 GB：与转换页同一对换算，模板里那个数就是方案里那个数
                return (
                    <Stepper
                        value={Math.round(Number(raw ?? 0) / 1024)}
                        min={1}
                        max={32}
                        suffix="GB"
                        readOnly={!on}
                        onChange={(v) => setOne(k, v * 1024)}
                    />
                );
            case "select":
                return (
                    <SearchSelect
                        className={w}
                        label={stacked ? m.label : undefined}
                        // javaPath 的空串是「自动」这一档，不是「没值」：它必须显式显示成「自动选择」
                        value={
                            !present
                                ? ""
                                : k === "javaPath"
                                  ? String(raw || JAVA_AUTO)
                                  : String(raw)
                        }
                        options={selectOptions(k)}
                        placeholder={notIn}
                        readOnly={!on}
                        onOpen={k === "javaPath" ? probeJava : undefined}
                        onChange={(v) => setOne(k, k === "javaPath" && v === JAVA_AUTO ? "" : v)}
                    />
                );
            case "number":
                return (
                    <NumInput
                        value={present ? Number(raw) : null}
                        min={k === "serverPort" ? SERVER_PORT_RANGE.min : 1}
                        max={k === "serverPort" ? SERVER_PORT_RANGE.max : 1000}
                        readOnly={!on}
                        placeholder={notIn}
                        onCommit={(v) => setOne(k, v)}
                    />
                );
            case "text":
                return (
                    <TextInput
                        className={w}
                        value={present ? String(raw) : ""}
                        readOnly={!on}
                        placeholder={present ? textPh(k) : notIn}
                        spellCheck={false}
                        onChange={(e) => setOne(k, e.target.value)}
                    />
                );
            case "path":
                return (
                    <PathBox
                        ticked={on}
                        present={present}
                        value={present ? String(raw) : ""}
                        onPick={async () => {
                            const dir = await api.pickDirectory();
                            if (dir) setOne(k, dir);
                        }}
                    />
                );
        }
    };

    /** 组计数芯片：`n / 总数`，启动参数那一张在结构信号成立时染金 */
    const groupChip = (g: TemplateGroup) => {
        const list = groupOf(g);
        return (
            <ToneChip tone={g === "launch" && scriptsDead ? "gold" : "muted"} mono>
                {`${list.filter((m) => isIn(m.key)).length} / ${list.length}`}
            </ToneChip>
        );
    };

    const save = async () => {
        if (!canSave) return;
        const at = templates.findIndex((x) => x.id === templateId);
        const base = at >= 0 ? templates[at] : undefined;
        const tpl: ConversionTemplate = {
            id: base?.id ?? api.newTemplateId(),
            name: trimmed,
            note: note.trim(),
            values: picked,
            updatedAt: Date.now(),
        };
        if (await commit(base ? templates.map((x) => (x.id === tpl.id ? tpl : x)) : [...templates, tpl])) {
            dirtyRef.current = false;
            setBaseSig(sigOf(tpl.name, tpl.note, tpl.values));
            notify(t("templates.saved-toast", "已保存模板 · {{count}} 项", { count }), "success");
            back();
        }
    };

    /** 另存为副本：把当前这份勾好的存成**新**模板，原模板不动 ⇒ 留在这一页，未保存标记照旧 */
    const saveCopy = async () => {
        if (!canSave || !guardTemplateCap(templates.length)) return;
        const copy: ConversionTemplate = {
            id: api.newTemplateId(),
            name: uniqueTemplateName(
                `${trimmed}${t("templates.copy-suffix", " 副本")}`,
                templates.map((x) => x.name)
            ),
            note: note.trim(),
            values: picked,
            updatedAt: Date.now(),
        };
        const next = [...templates];
        next.splice(templates.findIndex((x) => x.id === templateId) + 1, 0, copy);
        if (await commit(next)) {
            notify(t("templates.saved-as", "已另存为副本 · {{name}}", { name: copy.name }), "success");
        }
    };

    const remove = async () => {
        setPendingDelete(false);
        if (await commit(templates.filter((x) => x.id !== templateId))) {
            dirtyRef.current = false;
            back();
        }
    };

    return (
        <motion.div
            className="flex flex-col gap-5 overflow-hidden py-6"
            variants={PAGE_RISE}
            initial="hidden"
            animate="show"
        >
            {header}

            <motion.div variants={CARD_RISE} className="flex w-full items-start gap-5">
                {/* 左列：模板信息 + 三组字段（卡间 20，与转换页同族） */}
                <div className="flex min-w-0 flex-1 flex-col gap-5">
                    <Panel gap={14}>
                        <PanelHead title={t("templates.info", "模板信息")} />
                        <div className="flex flex-col gap-1.5">
                            <span className="text-[11px] leading-[16px] font-medium text-text-2">
                                {t("templates.name", "模板名称")}
                            </span>
                            <TextInput
                                className="w-full"
                                value={name}
                                invalid={dupe}
                                spellCheck={false}
                                maxLength={TEMPLATE_NAME_MAX}
                                placeholder={t("templates.name-ph", "如：生存服预设")}
                                onChange={(e) => patchDraft({ name: e.target.value })}
                            />
                            {/* 拦保存的判据与这行红字同源（都问 `dupe`）：不会出现染了红却还能按 */}
                            {dupe && (
                                <span className="font-mono text-[11px] leading-[16px] font-normal text-redstone">
                                    {t("templates.duplicate-name", "已有同名模板")}
                                </span>
                            )}
                        </div>
                        <div className="flex flex-col gap-1.5">
                            <span className="text-[11px] leading-[16px] font-medium text-text-2">
                                {t("templates.note-label", "备注（选填）")}
                            </span>
                            <TextInput
                                className="w-full"
                                value={note}
                                spellCheck={false}
                                maxLength={TEMPLATE_NOTE_MAX}
                                placeholder={t("templates.note-ph", "如：每周六开 · 结束后不停服")}
                                onChange={(e) => patchDraft({ note: e.target.value })}
                            />
                        </div>
                    </Panel>

                    <Panel gap={14}>
                        <PanelHead
                            title={t("convert.runtime", "运行环境")}
                            right={groupChip("runtime")}
                        />
                        {groupOf("runtime").map((m) => (
                            <FieldRow
                                key={m.key}
                                meta={m}
                                ticked={isIn(m.key)}
                                isNew={isNew}
                                onToggle={(v) => tick(m.key, v)}
                            >
                                {controlOf(m)}
                            </FieldRow>
                        ))}
                    </Panel>

                    <Panel gap={14}>
                        <PanelHead
                            title={t("convert.launch-args", "启动参数")}
                            right={groupChip("launch")}
                        />
                        {groupOf("launch").map((m) => (
                            <FieldRow
                                key={m.key}
                                meta={m}
                                ticked={isIn(m.key)}
                                isNew={isNew}
                                onToggle={(v) => tick(m.key, v)}
                            >
                                {controlOf(m)}
                            </FieldRow>
                        ))}
                    </Panel>

                    {/* 服务端设置这七档的版式跟转换页那张卡对齐：两枚下拉并排、端口与人数两格、
                        MOTD 与种子整宽、正版验证收在分隔线下方 */}
                    <Panel gap={14}>
                        <PanelHead
                            title={t("convert.server-settings", "服务端设置")}
                            right={groupChip("server")}
                        />
                        <div className="flex w-full gap-3">
                            {(["gamemode", "difficulty"] as TemplateFieldKey[]).map((k) => (
                                <FieldRow
                                    key={k}
                                    meta={byKey[k]}
                                    ticked={isIn(k)}
                                    isNew={isNew}
                                    stacked
                                    className="flex-1"
                                    onToggle={(v) => tick(k, v)}
                                >
                                    {controlOf(byKey[k], true)}
                                </FieldRow>
                            ))}
                        </div>
                        <div className="grid w-full grid-cols-2 gap-3">
                            {(["serverPort", "maxPlayers"] as TemplateFieldKey[]).map((k) => (
                                <FieldRow
                                    key={k}
                                    meta={byKey[k]}
                                    ticked={isIn(k)}
                                    isNew={isNew}
                                    stacked
                                    onToggle={(v) => tick(k, v)}
                                >
                                    {controlOf(byKey[k], true)}
                                </FieldRow>
                            ))}
                        </div>
                        {(["motd", "levelSeed"] as TemplateFieldKey[]).map((k) => (
                            <FieldRow
                                key={k}
                                meta={byKey[k]}
                                ticked={isIn(k)}
                                isNew={isNew}
                                stacked
                                onToggle={(v) => tick(k, v)}
                            >
                                {controlOf(byKey[k], true)}
                            </FieldRow>
                        ))}
                        <Divider />
                        <FieldRow
                            meta={byKey.onlineMode}
                            ticked={isIn("onlineMode")}
                            isNew={isNew}
                            onToggle={(v) => tick("onlineMode", v)}
                        >
                            {controlOf(byKey.onlineMode)}
                        </FieldRow>
                    </Panel>
                </div>

                {/* 右栏 280px：保存只在这一处（页头那格放脏标记），读数与出口同列 */}
                <aside className="flex w-[280px] shrink-0 flex-col gap-5">
                    <Panel gap={14}>
                        <PanelHead title={t("templates.summary", "模板摘要")} />
                        {/* 0 是新建那一屏唯一的信号：不弹提示、不给气泡，染金 + 保存禁用就是反馈 */}
                        <CountRow
                            label={t("templates.included", "纳入模板")}
                            count={count}
                            tone={count === 0 ? "gold" : "accent"}
                        />
                        <CountRow
                            label={t("templates.not-included", "未纳入")}
                            count={TEMPLATE_FIELD_COUNT - count}
                            tone="muted"
                        />
                        <Divider />
                        {/* 「为什么不收 MC 版本」这问题第一次进来就会问：当数据摆在读数下面，
                            而不是当一句解释散进各张字段卡（全页的解释只留页头那一句） */}
                        <SectionTitle>{t("templates.excluded-head", "不进模板")}</SectionTitle>
                        <p className="font-mono text-[11px] leading-[16px] font-normal text-text-3">
                            {t(
                                "templates.excluded-list",
                                "MC 版本 · 加载器版本 · Java 需求线 · 模组方案 · 包内保留内容 · 缺件放行"
                            )}
                        </p>
                        <Divider />
                        <Btn
                            variant="primary"
                            full
                            icon={Save}
                            disabled={!canSave}
                            onClick={() => void save()}
                        >
                            {t("templates.save", "保存模板")}
                        </Btn>
                        {!isNew && (
                            <Btn size="sm" full onClick={() => void saveCopy()}>
                                {t("templates.save-as-copy", "另存为副本")}
                            </Btn>
                        )}
                        {!isNew && (
                            <div className="flex w-full justify-center">
                                {/* 删除这一族全站都用 redstone 说话（列表卡那颗按钮、弹窗里那颗实心钮）：
                                    这一枚从前压成三级灰，反而成了三处里最不显眼的一个 */}
                                <LinkBtn size="sm" className="text-redstone" onClick={() => setPendingDelete(true)}>
                                    {t("templates.delete-title", "删除模板")}
                                </LinkBtn>
                            </div>
                        )}
                        <p className="w-full text-center font-mono text-[10px] leading-[14px] font-normal text-text-3">
                            {canSave
                                ? t("templates.foot-edit", "只写参数 · 存本地配置目录 · 删模板不影响已建任务")
                                : t("templates.foot-new", "勾至少一行、填个名字才能存")}
                        </p>
                    </Panel>
                </aside>
            </motion.div>

            <DeleteTemplateModal
                template={pendingDelete ? (target ?? null) : null}
                onClose={() => setPendingDelete(false)}
                onConfirm={() => void remove()}
            />
        </motion.div>
    );
}

/* ---------------- 行与控件小件 ---------------- */

/**
 * 一行的两种版式（稿内 `.frow`）：勾选框 + 文字盒 +（右端控件 或 下方控件）。
 *
 * 勾选框在两种版式里都是**垂直居中**的（`.frow{align-items:center}` 而不是 flex-start）：
 * 它属于整行而不属于那一行标签，压在标签顶上看起来像「勾的是标题」。
 * 标签三档色：勾中 = $text-1；未勾且新建 = $text-2（值是等着被带走的）；未勾且编辑 = $text-3（这档不在模板里）。
 * `stacked` 时小标用 11/16/500 $text-2（与转换页 `Field` 同一档），而下拉自己带 11/16/400 $text-3 那档
 * ⇒ 那一行的标签不再另画，免得同两行字出现两种字号。
 */
function FieldRow({
    meta,
    ticked,
    isNew,
    stacked,
    className,
    onToggle,
    children,
}: {
    meta: TemplateFieldMeta;
    ticked: boolean;
    isNew: boolean;
    stacked?: boolean;
    className?: string;
    onToggle: (v: boolean) => void;
    children: ReactNode;
}) {
    return (
        <div className={cn("flex w-full items-center gap-2.5", className)}>
            <CheckBox checked={ticked} onChange={onToggle} />
            <span
                className={cn(
                    "flex min-w-0 flex-1 flex-col",
                    stacked ? "gap-1.5" : meta.hint ? "gap-0.5" : undefined
                )}
            >
                {!stacked || meta.kind !== "select" ? (
                    stacked ? (
                        <span className="text-[11px] leading-[16px] font-medium text-text-2">
                            {meta.label}
                        </span>
                    ) : (
                        <span
                            className={cn(
                                "text-[12px] leading-[18px] font-medium",
                                ticked ? "text-text-1" : isNew ? "text-text-2" : "text-text-3"
                            )}
                        >
                            {meta.label}
                        </span>
                    )
                ) : null}
                {meta.hint && (
                    <span className="font-mono text-[11px] leading-[16px] font-normal text-text-3">
                        {meta.hint}
                    </span>
                )}
                {stacked && children}
            </span>
            {!stacked && children}
        </div>
    );
}

/** 数字那一格：本地草稿容得下半成品，失焦才吸回区间（口径同转换页 `NumField`，但没值时显示「未纳入」） */
function NumInput({
    value,
    min,
    max,
    readOnly,
    placeholder,
    onCommit,
}: {
    value: number | null;
    min: number;
    max: number;
    readOnly?: boolean;
    placeholder: string;
    onCommit: (v: number) => void;
}) {
    const [txt, setTxt] = useState(value === null ? "" : String(value));
    // 只在外部换值时对齐草稿（勾上一行填来默认值 / 另存后回读）；自己打字绕回来的一圈别把 "07" 改成 "7"
    useEffect(
        () => setTxt((d) => (parseInt(d, 10) === value ? d : value === null ? "" : String(value))),
        [value]
    );
    const digits = (raw: string) => {
        const n = parseInt(raw, 10);
        return Number.isFinite(n) ? n : null;
    };
    return (
        <TextInput
            className="w-full"
            inputMode="numeric"
            value={txt}
            readOnly={readOnly}
            placeholder={placeholder}
            onChange={(e) => {
                const d = e.target.value.replace(/\D/g, "").slice(0, 6);
                setTxt(d);
                const n = digits(d);
                if (n !== null) onCommit(n);
            }}
            onBlur={() => {
                const n = digits(txt);
                const c = n === null ? (value ?? min) : Math.min(max, Math.max(min, n));
                setTxt(String(c));
                onCommit(c);
            }}
        />
    );
}

/**
 * 输出目录那一格：不给打字，点了开系统目录选择器（与转换页「本次改用其他目录」同一条路）。
 * 三档读数各有各的意思：路径（等宽）= 本次覆写；空串 = 「跟随全局」（勾了但没覆写）；
 * 没有值 = 「未纳入」（这档压根不在模板里）。未勾时整格按只读口径灰化，点了也不开框。
 */
function PathBox({
    ticked,
    present,
    value,
    onPick,
}: {
    ticked: boolean;
    present: boolean;
    value: string;
    onPick: () => void | Promise<void>;
}) {
    const t = useT();
    return (
        <button
            type="button"
            onClick={() => ticked && void onPick()}
            className={cn(
                "flex h-8 w-[200px] shrink-0 items-center gap-[7px] rounded-lg border px-2.5 text-left",
                // 可点那一档与 `TextInput` 卡态同底（$surface-2 无描边），悬停只把底色提一档
                ticked
                    ? "border-transparent bg-surface-2 text-text-1 hover:bg-surface-2/70"
                    : READONLY_BOX
            )}
        >
            <Folder className="size-3 shrink-0 text-text-3" />
            <span
                className={cn(
                    "min-w-0 truncate",
                    // 路径要认得出是哪一格目录 ⇒ 等宽；两句读数是正文（跟下拉空值那格同一口径）
                    value ? "font-mono text-[12px] leading-[18px] font-medium" : "text-[13px] leading-[20px] font-normal"
                )}
            >
                {value
                    ? truncateMiddle(value, 24)
                    : present
                      ? t("templates.follow-global", "跟随全局")
                      : t("templates.not-included", "未纳入")}
            </span>
        </button>
    );
}
