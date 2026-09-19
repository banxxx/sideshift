/**
 * Idle 帮助说明三卡（SS.pen Home·Idle `SjdZR` help-row，v6.1 定稿）
 *
 * 用户否决过"最近转换列表"方案，此处只做静态说明，勿加交互功能。
 * 动画走 variants 层级：idle 分支挂载时由父级 delayChildren 触发，
 * 三卡自左向右错峰上浮入场；退场逆序下沉，与整体淡出同步收尾。
 */
import { motion, type Variants } from "motion/react";
import { Archive, Folder, Server, type LucideIcon } from "lucide-react";

const row: Variants = {
    hidden: {},
    show: { transition: { staggerChildren: 0.07 } },
    hide: { transition: { staggerChildren: 0.03, staggerDirection: -1 } },
};

const card: Variants = {
    hidden: { opacity: 0, y: 24 },
    show: {
        opacity: 1,
        y: 0,
        transition: { type: "spring", stiffness: 320, damping: 28 },
    },
    hide: { opacity: 0, y: 18, transition: { duration: 0.15 } },
};

const HELPS: Array<{ icon: LucideIcon; title: string; desc: string }> = [
    {
        icon: Archive,
        title: "会剔除什么",
        desc: "光影、小地图、键鼠等客户端专属模组与资源残留",
    },
    {
        icon: Server,
        title: "会补齐什么",
        desc: "Fabric API 等服务端依赖，Forge 运行时库",
    },
    {
        icon: Folder,
        title: "输出到哪里",
        desc: "生成同名服务端包，一键打开目录或拷贝至服务器",
    },
];

export function HelpRow() {
    return (
        <motion.div variants={row} className="w-[760px] flex gap-4">
            {HELPS.map(({ icon: Icon, title, desc }) => (
                <motion.div
                    key={title}
                    variants={card}
                    className="flex-1 h-[88px] bg-surface border border-stroke rounded-[12px] px-4 py-3.5 flex flex-col gap-1.5"
                >
                    <div className="flex items-center gap-1.5">
                        <Icon className="size-3.5 text-accent" />
                        <span className="text-xs font-semibold text-text-1">{title}</span>
                    </div>
                    <p className="text-[11px] leading-[1.5] text-text-3">{desc}</p>
                </motion.div>
            ))}
        </motion.div>
    );
}
