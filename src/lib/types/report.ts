/** 转换报告（对应 Report 屏） */
import type { ConversionOptions } from "./options";

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
    /** 启动脚本指向的 jar 名：Fabric 为服务端 jar，Forge/NeoForge 为 installer */
    startJar?: string;
}
