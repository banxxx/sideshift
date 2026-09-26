/** 转换报告（对应 Report 屏） */
import type { BackendMsg } from "./l10n";
import type { ConversionOptions } from "./options";

/** 构建后静态自检的三态（对应 core::verify 的 CheckStatus） */
export type CheckStatus = "pass" | "warn" | "fail";

/** 自检单项结果：只核对「产物齐不齐、坏没坏」，措辞不得写成「校验通过 = 可开服」 */
export interface CheckResult {
    /** 稳定标识：files / jars / deps / start / loader / root / keep —— 按它排布，不认中文标题 */
    id: string;
    label: string;
    status: CheckStatus;
    /** 一句话结论（带真实数字）；后端渲染好的中文整句，也是 `detailMsg.zh` */
    detail: string;
    /** 同一句话的「模板 + 参数」，界面按它查翻译目录。旧任务快照没有这一项 ⇒ undefined */
    detailMsg?: BackendMsg | null;
    /** 涉及的对象名（缺哪些文件、哪几个 jar 坏了），后端已截断 */
    items?: string[];
}

export interface ConversionReport {
    taskId: string;
    outputFileName: string;
    outputSizeBytes: number;
    /** 耗时（秒） */
    durationSec: number;
    /** 变更计数 */
    removed: number;
    kept: number;
    added: number;
    /** 待人工确认项 */
    pendingReview: string[];
    options: ConversionOptions;
    /** 打进 zip 的文件数（builder 实数） */
    fileCount: number;
    /** 本次实际写入包根的文件（start.bat / eula.txt / server.properties / …） */
    generatedFiles: string[];
    /** 启动脚本指向的 jar 名：Fabric 为服务端 jar，未本机安装的 Forge/NeoForge 为 installer；
     *  已装好的新式布局为 undefined（起跳靠 libraries/ 下的参数文件，没有单一 jar 可指） */
    startJar?: string;
    /** 本次把本机装好的 loader 树并进了产物：目标机不用再联网首装（Java 仍然要有） */
    installed: boolean;
    /** 构建后自检结论；空数组 = 未开启该开关 */
    checks: CheckResult[];
}
