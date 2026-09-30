/**
 * 品牌标识：一律用 public/logo.svg 单一图形源，不再各处自绘（画两份就会改一处忘一处）。
 * draggable=false：标题栏整条是拖拽区，图片默认允许原生拖拽会把窗口拖动吞掉。
 */
export function Logo({ className = "size-4" }: { className?: string }) {
    return (
        <img
            src="/logo.svg"
            alt=""
            aria-hidden
            draggable={false}
            className={className}
        />
    );
}
