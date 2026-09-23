/**
 * 品牌标识：一律用 public/logo.svg，不再各处自绘方块。
 *
 * 自绘的代价实测过两次：一次是上下两截错接成同一个主题色（深色下糊成平块），
 * 一次是壳与主应用各画一份、改一处忘一处。图形单源之后深浅两版长得一样正是想要的结果
 * ——图片本身没有主题分支，所以也不会再出现"切了主题 logo 就变了"。
 *
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
