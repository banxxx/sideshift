/**
 * 日志一键复制：图标态 Copy → Check（emerald），1.8s 后复位。
 *
 * 复制成功的反馈刻意留在按钮本体而不走全局提示区——日志复制是高频动作，
 * 与「删除行不发 toast」同一口径：动作结果就地可见。
 */
import { useRef, useState } from "react";
import { Check, Copy } from "lucide-react";
import { Btn, IconBtn } from "@/components/design/ui";
import { logsToClipText, type ClipLog } from "@/lib/log-view";
import { notify } from "@/lib/notify";

interface Props {
    logs: ClipLog[];
    /** 粘贴内容首行（任务/包名等上下文） */
    header?: string;
    /** labeled=头部文字按钮；floating=控制台右上角悬浮图标 */
    variant?: "labeled" | "floating";
    className?: string;
}

const RESET_MS = 1800;

export function LogCopyButton({ logs, header, variant = "labeled", className }: Props) {
    const [copied, setCopied] = useState(false);
    const timer = useRef(0);

    const copy = async () => {
        try {
            await navigator.clipboard.writeText(logsToClipText(logs, header));
        } catch {
            notify("复制失败：剪贴板不可用", "error");
            return;
        }
        window.clearTimeout(timer.current);
        setCopied(true);
        timer.current = window.setTimeout(() => setCopied(false), RESET_MS);
    };

    const empty = logs.length === 0;

    if (variant === "floating") {
        return (
            <IconBtn
                icon={copied ? Check : Copy}
                onClick={() => void copy()}
                disabled={empty}
                title={empty ? "暂无日志" : "复制全部日志"}
                className={className}
            />
        );
    }
    return (
        <Btn
            size="xs"
            variant="ghost"
            icon={copied ? Check : Copy}
            onClick={() => void copy()}
            disabled={empty}
            title={empty ? "暂无日志" : "复制为文本，可直接粘贴"}
        >
            {copied ? "已复制" : "复制"}
        </Btn>
    );
}
