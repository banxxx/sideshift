/**
 * 关于页 About（侧栏一级页，样片 `.scratch/about-proto.html`）
 *
 * 五块：品牌卡 / 鸣谢名单（密排卡 + 高度档）/ 数据与隐私（可展开）/ 第三方组件（可展开）/ 声明。
 *
 * **内容单一**这条是硬约束：版本号、预发布徽章、检查更新、更新渠道、主题、语言**只在设置出现**，
 * 这里一处都不放（样片里那行「按首次贡献时间排序」也删了——顺序就是远端那份名单的数组顺序，
 * 写一句和实现无关的说明等于给自己埋一条会过期的文案）。页头不留标题：品牌卡已经把
 * 「这是关于页」说清楚了，侧栏那行高亮也在说同一件事。
 *
 * 名单不写死在包里：`useContributors` 先读本机快照上屏、后台向远端对账一次，
 * 拿不到就在这一块内嵌一句「不可见 + 重新获取」（不抢全局提示区）。
 *
 * 两块可展开走公共 `Collapse`（三条硬规矩：壳必须空、`gap` 传父级格距、overflow 演完才撤），
 * 符号统一用 `FoldBtn`（下拉选择框那枚 chevron，开合由 `aria-expanded` 外显）。
 * 密排卡与它的高度档整套在 `AckWall` 里，理由写在那儿。
 *
 * 「源码仓库」是 `REPO_URL` 的入口（设置页那条已撤下，等这一页接回）。
 */
import { useState, type ReactNode } from "react";
import { motion } from "motion/react";
import { ExternalLink } from "lucide-react";
import * as api from "@/lib/api";
import { CARD_RISE, PAGE_RISE } from "@/lib/page-motion";
import { useT, type TranslateFn } from "@/lib/i18n";
import { cn } from "@/lib/utils";
import { Btn, Collapse, Divider, FoldBtn, Logo, Panel, PanelHead } from "@/components/ui";
import { AckWall } from "./AckWall";
import { useContributors } from "./useContributors";

/** 数据与隐私。左边是字段名、右边是一句完整的话——都走 `t`，表建在函数里（顶层建表会把词冻在首次加载的语言上） */
function privacyRows(t: TranslateFn): Array<[string, string]> {
    return [
        [t("about.process", "处理方式"), t("about.process-value", "整合包的解包、判定与重建全部在本机执行，包体内容不作上传")],
        [t("about.network-scope", "联网范围"), t("about.network-scope-value", "仅三种情形主动发起请求：模组与版本元数据补全、应用更新检查、鸣谢名单获取")],
        [t("about.data-source", "数据来源"), t("about.data-source-value", "Modrinth API、CurseForge API，以及在设置中指定的镜像服务")],
        [t("about.ack-source", "鸣谢名单"), t("about.ack-source-value", "名单与自带头像取自项目自管的静态地址；登记了 Minecraft 玩家名的贡献者，其皮肤由本机向 Mojang 公开档案接口查询、贴图直连其 CDN 取回。这些请求不携带账号、凭据或本地文件信息；名单快照与皮肤副本留存于本机配置目录，供离线显示")],
        [t("about.telemetry", "遥测统计"), t("about.telemetry-value", "未集成遥测或统计上报组件，亦不写入本地统计数据文件")],
        [t("about.credentials", "凭据存储"), t("about.credentials-value", "CurseForge API Key 仅保存于本机配置，不进入日志、任务存档与导出产物")],
    ];
}

/**
 * 第三方组件与许可。`name` 与 `license` 是专有名词与 SPDX 标识，**不翻**；
 * 只有用途那句过 `t`。Minecraft 的权利声明不在这张表里——下面「声明」那一块已经写了，
 * 同一页两处重复只会带来两处各改一次的风险。
 */
function thirdPartyRows(t: TranslateFn): Array<[string, string, string]> {
    return [
        ["Fabric · Forge · NeoForge", t("about.tp-loaders", "模组加载器生态，转换方案判定所依据的公开规范"), "Apache-2.0 / LGPL"],
        ["Modrinth API · CurseForge API", t("about.tp-meta", "模组与版本元数据来源"), t("about.tp-public-api", "公开 API")],
        ["BMCLAPI（麦块）", t("about.tp-mirror", "可选的下载与元数据镜像加速服务"), "MIT"],
        ["Tauri 2", t("about.tp-tauri", "桌面运行时与原生能力封装"), "MIT / Apache-2.0"],
        ["React 19 · motion", t("about.tp-react", "界面框架与动效引擎"), "MIT"],
        ["shadcn/ui · Tailwind CSS 4 · lucide", t("about.tp-ui", "组件基线、样式系统与图标集"), "MIT"],
        ["Inter · Space Grotesk · JetBrains Mono", t("about.tp-fonts", "界面字体，随安装包内嵌分发"), "SIL OFL 1.1"],
    ];
}

/** 字段名 + 一句话：关于页两块可展开共用的排法（窄栏定宽、正文吃剩下的宽度并换行） */
function FactRow({ label, children }: { label: string; children: ReactNode }) {
    return (
        <div className="flex items-start gap-3">
            <span className="w-[76px] shrink-0 text-[11px] leading-[18px] text-text-3">{label}</span>
            <span className="min-w-0 flex-1 text-[12px] leading-[18px] text-text-1">{children}</span>
        </div>
    );
}

/** 展开后那一叠内容的外壳：分隔线与内边距都画在壳**内**（公共 Collapse 的第 1 条：壳必须空） */
function FoldBody({ children }: { children: ReactNode }) {
    return (
        <div className="flex flex-col gap-3">
            <Divider />
            {children}
        </div>
    );
}

export function AboutPage() {
    const t = useT();
    const { people, skins, status, retry } = useContributors();
    const [privacyOpen, setPrivacyOpen] = useState(false);
    const [thirdPartyOpen, setThirdPartyOpen] = useState(false);

    return (
        <motion.div
            variants={PAGE_RISE}
            initial="hidden"
            animate="show"
            className="flex flex-col gap-5 py-6"
        >
            {/* ---- 品牌 ---- */}
            <motion.div variants={CARD_RISE}>
                <Panel>
                    <div className="flex w-full items-center gap-4">
                        <Logo className="size-11 shrink-0" />
                        <div className="flex min-w-0 flex-col gap-0.5">
                            <span className="font-heading text-[17px] leading-[22px] font-bold text-text-1">
                                SideShift
                            </span>
                            <span className="text-[11px] leading-[16px] text-text-3">
                                {t("about.positioning", "独立第三方工具 · MIT 协议 · 无遥测")}
                            </span>
                        </div>
                        <Btn
                            size="sm"
                            icon={ExternalLink}
                            className="ml-auto"
                            onClick={() => void api.openExternal(api.REPO_URL)}
                        >
                            {t("about.source-repo", "源码仓库")}
                        </Btn>
                    </div>
                </Panel>
            </motion.div>

            {/* ---- 鸣谢名单 ---- */}
            <motion.div variants={CARD_RISE}>
                <AckWall people={people} skins={skins} status={status} onRetry={retry} />
            </motion.div>

            {/* ---- 数据与隐私 ---- */}
            <motion.div variants={CARD_RISE}>
                {/* 收起态只有一行表头 ⇒ 上下内边距收一档（20 → 12），见 `Panel` 的 `padY` */}
                <Panel gap={10} padY={12}>
                    <PanelHead
                        title={t("about.privacy", "数据与隐私")}
                        right={
                            <FoldBtn
                                open={privacyOpen}
                                label={
                                    privacyOpen
                                        ? t("about.collapse-privacy", "收起数据与隐私")
                                        : t("about.expand-privacy", "展开数据与隐私")
                                }
                                onClick={() => setPrivacyOpen((v) => !v)}
                            />
                        }
                    />
                    <Collapse when={privacyOpen} gap={10}>
                        <FoldBody>
                            {privacyRows(t).map(([label, value]) => (
                                <FactRow key={label} label={label}>
                                    {value}
                                </FactRow>
                            ))}
                        </FoldBody>
                    </Collapse>
                </Panel>
            </motion.div>

            {/* ---- 第三方组件 ---- */}
            <motion.div variants={CARD_RISE}>
                {/* 收起态只有一行表头 ⇒ 上下内边距收一档（20 → 12），见 `Panel` 的 `padY` */}
                <Panel gap={10} padY={12}>
                    <PanelHead
                        title={t("about.third-party", "第三方组件")}
                        right={
                            <FoldBtn
                                open={thirdPartyOpen}
                                label={
                                    thirdPartyOpen
                                        ? t("about.collapse-third-party", "收起第三方组件")
                                        : t("about.expand-third-party", "展开第三方组件")
                                }
                                onClick={() => setThirdPartyOpen((v) => !v)}
                            />
                        }
                    />
                    <Collapse when={thirdPartyOpen} gap={10}>
                        <FoldBody>
                            {thirdPartyRows(t).map(([name, usage, license]) => (
                                <div key={name} className="flex items-start gap-3">
                                    <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                                        <span className="text-[12px] leading-[18px] font-semibold text-text-1">
                                            {name}
                                        </span>
                                        <span className="text-[11px] leading-[16px] text-text-3">{usage}</span>
                                    </div>
                                    <span
                                        className={cn(
                                            "shrink-0 rounded-full bg-surface-2 px-2 py-0.5",
                                            "font-mono text-[10px] leading-[14px] font-semibold text-text-2"
                                        )}
                                    >
                                        {license}
                                    </span>
                                </div>
                            ))}
                        </FoldBody>
                    </Collapse>
                </Panel>
            </motion.div>

            {/* ---- 声明 ---- */}
            <motion.div variants={CARD_RISE}>
                <Panel gap={6}>
                    <p className="text-[12px] leading-[20px] text-text-2">
                        {t(
                            "about.disclaimer",
                            "SideShift 是独立的第三方工具，与 Mojang Studios 或 Microsoft 无任何隶属或授权关系。Minecraft 及其相关名称、素材为相应权利人的商标或版权作品。本项目以 MIT 协议发布，不含任何官方内容；用于转换你所持有的整合包，责任自负。"
                        )}
                    </p>
                    <p className="font-mono text-[11px] leading-[16px] text-text-3">
                        © {new Date().getFullYear()} SideShift contributors
                    </p>
                </Panel>
            </motion.div>
        </motion.div>
    );
}
