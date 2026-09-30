/**
 * 关于页 About（侧栏一级页）：品牌卡 / 鸣谢名单（密排卡 + 高度档）/ 数据与隐私 / 许可与依赖 / 声明。
 * **内容单一**是硬约束：版本号、检查更新、更新渠道、主题、语言**只在设置出现**，这里一处都不放；页头不留标题。
 * 名单不写死在包里：`useContributors` 先读本机快照上屏、后台向远端对账一次；失败/空态的实话与重试出口在 `AckWall` 内。
 * 两块可展开走公共 `Collapse`（壳必须空、`gap` 传父级格距、overflow 演完才撤），符号统一用 `FoldBtn`。
 */
import { useState, type ReactNode } from "react";
import { motion } from "motion/react";
import {
    Database,
    ExternalLink,
    KeyRound,
    Laptop,
    ShieldOff,
    Users,
    Wifi,
    type LucideIcon,
} from "lucide-react";
import * as api from "@/lib/api";
import { CARD_RISE, PAGE_RISE } from "@/lib/page-motion";
import { useT, type TranslateFn } from "@/lib/i18n";
import { cn } from "@/lib/utils";
import { Btn, Collapse, Divider, FoldBtn, Logo, Panel, PanelHead } from "@/components/ui";
import { AckWall } from "./AckWall";
import { useContributors } from "./useContributors";

/**
 * 数据与隐私。图标 + 字段名 + 一句话：六行都是「本机做了什么」的陈述，一枚图标换一行，
 * 让这块读起来是一份清单而不是六段散文。左边是字段名、右边是一句完整的话——都走 `t`，
 * 表建在函数里（顶层建表会把词冻在首次加载的语言上）。
 */
function privacyRows(t: TranslateFn): Array<[LucideIcon, string, string]> {
    return [
        [Laptop, t("about.process", "处理方式"), t("about.process-value", "整合包的解包、判定与重建全部在本机执行，包体内容不作上传")],
        [Wifi, t("about.network-scope", "联网范围"), t("about.network-scope-value", "仅三种情形主动发起请求：模组与版本元数据补全、应用更新检查、鸣谢名单获取")],
        [Database, t("about.data-source", "数据来源"), t("about.data-source-value", "Modrinth API、CurseForge API，以及在设置中指定的镜像服务")],
        [Users, t("about.ack-source", "鸣谢名单"), t("about.ack-source-value", "名单与自带头像取自项目自管的静态地址；登记了 Minecraft 玩家名的贡献者，其皮肤由本机向 Mojang 公开档案接口查询、贴图直连其 CDN 取回。这些请求不携带账号、凭据或本地文件信息；名单快照与皮肤副本留存于本机配置目录，供离线显示")],
        [ShieldOff, t("about.telemetry", "遥测统计"), t("about.telemetry-value", "未集成遥测或统计上报组件，亦不写入本地统计数据文件")],
        [KeyRound, t("about.credentials", "凭据存储"), t("about.credentials-value", "CurseForge API Key 仅保存于本机配置，不进入日志、任务存档与导出产物")],
    ];
}

/**
 * 许可与依赖。只留**外部**的三项（加载器生态、元数据 API、镜像服务）加本项目自身的发布协议——
 * `name` 与 `license` 是专有名词与 SPDX 标识，**不翻**，只有用途那句过 `t`。
 *
 * 应用栈（运行时/框架/组件基线/字体）不列在这里：这一页不是 NOTICE 文件，把工程依赖摊给用户看
 * 只增加阅读成本。Minecraft 的权利声明也不在这张表里——下面「声明」那一块已经写了，
 * 同一页两处重复只会带来两处各改一次的风险。
 */
function licenseRows(t: TranslateFn): Array<[string, string, string]> {
    return [
        ["Fabric · Forge · NeoForge", t("about.tp-loaders", "模组加载器生态，转换方案判定所依据的公开规范"), "Apache-2.0 / LGPL"],
        ["Modrinth API · CurseForge API", t("about.tp-meta", "模组与版本元数据来源"), t("about.tp-public-api", "公开 API")],
        ["BMCLAPI（麦块）", t("about.tp-mirror", "可选的下载与元数据镜像加速服务"), "MIT"],
        ["MIT License", t("about.tp-self", "本项目自身的发布协议"), "MIT"],
    ];
}

/** 图标 + 字段名 + 一句话：隐私那六行共用的排法（图标列与字段名定宽，正文吃剩下的宽度并换行） */
function FactRow({ icon: Icon, label, children }: { icon: LucideIcon; label: string; children: ReactNode }) {
    return (
        <div className="flex items-start gap-3">
            <span className="flex h-[18px] w-[14px] shrink-0 items-center justify-center text-text-3">
                <Icon className="size-[13px]" strokeWidth={1.75} />
            </span>
            <span className="w-[76px] shrink-0 text-[11px] leading-[18px] text-text-3">{label}</span>
            <span className="min-w-0 flex-1 text-[12px] leading-[18px] text-text-1">{children}</span>
        </div>
    );
}

/** 依赖行：名称 + 用途竖排，许可标贴右端（两列网格里各列自己占一格，所以不套 FactRow） */
function LicenseRow({ name, usage, license }: { name: string; usage: string; license: string }) {
    return (
        <div className="flex min-w-0 items-start gap-3">
            <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                <span className="text-[12px] leading-[18px] font-semibold text-text-1">{name}</span>
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
    const { people, skins, skinReady, status, retry } = useContributors();
    const [privacyOpen, setPrivacyOpen] = useState(false);
    const [licenseOpen, setLicenseOpen] = useState(false);

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
                <AckWall people={people} skins={skins} skinReady={skinReady} status={status} onRetry={retry} />
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
                            {privacyRows(t).map(([icon, label, value]) => (
                                <FactRow key={label} icon={icon} label={label}>
                                    {value}
                                </FactRow>
                            ))}
                        </FoldBody>
                    </Collapse>
                </Panel>
            </motion.div>

            {/* ---- 许可与依赖 ---- */}
            <motion.div variants={CARD_RISE}>
                {/* 收起态只有一行表头 ⇒ 上下内边距收一档（20 → 12），见 `Panel` 的 `padY` */}
                <Panel gap={10} padY={12}>
                    <PanelHead
                        title={t("about.license-deps", "许可与依赖")}
                        right={
                            <FoldBtn
                                open={licenseOpen}
                                label={
                                    licenseOpen
                                        ? t("about.collapse-license-deps", "收起许可与依赖")
                                        : t("about.expand-license-deps", "展开许可与依赖")
                                }
                                onClick={() => setLicenseOpen((v) => !v)}
                            />
                        }
                    />
                    <Collapse when={licenseOpen} gap={10}>
                        <FoldBody>
                            {/* 两列并排（等分 fr，不写固定宽）：四条正好 2×2，读起来是一份对照表
                                而不是四行清单；窗口最小 1120 时单列约容 21 字，最长的用途句不换行 */}
                            <div className="grid grid-cols-2 gap-x-6 gap-y-3">
                                {licenseRows(t).map(([name, usage, license]) => (
                                    <LicenseRow key={name} name={name} usage={usage} license={license} />
                                ))}
                            </div>
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
