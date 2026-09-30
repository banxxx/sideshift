/**
 * 悬浮填充动效 HOVER_FILL —— 「悬浮选中态」灰色底的出现/消失节奏，全应用统一挂这个常量，改观感只改这一处。
 * 覆盖 background-color / color / border-color / opacity；出现 180ms ease-out、移开 200ms ease-in（非 hover 态那份参数即退场曲线）。
 * 留在 CSS 而不用 motion 驱动：底色由主题令牌给出，抄成 JS 字面量主题翻面即失真。
 * 例外：Dropzone 拖放卡不挂本常量——它自带的 200ms 过渡与卡片上浮弹簧是一体调校，不参与统一。
 */
export const HOVER_FILL = [
    "transition-[background-color,color,border-color,opacity]",
    "duration-200 ease-[cubic-bezier(0.4,0,0.6,1)]",
    "hover:duration-180 hover:ease-[cubic-bezier(0.33,1,0.68,1)]",
].join(" ");

/**
 * 按压反馈 HOVER_PRESS = HOVER_FILL + transform（按下缩到 0.98）——按钮族专用。
 *
 * 为什么单开一个常量而不是往 HOVER_FILL 里加 transform：那一列属性表是**所有**挂件套件共用的，
 * 行类控件（ListRow / ChangeRow / 侧栏导航）加了缩放会整条 700px 宽的边挪 7px，读起来是「页面抖」
 * 而不是「按下了」。按钮自己的 0.98 只有一两 px，刚好够指尖确认。
 *
 * 为什么用 CSS 而不是 motion 的 whileTap：那要把 Btn/IconBtn 全换成 motion.button（全站几十个用点
 * 的宿主节点都换引擎），而按压缩放和底色一样是「元素本来就有的状态反馈」，CSS 一行就够；
 * 换成 motion 还要处理 props 类型与 layout 投影的相互干涉，收益是零。
 *
 * 节拍：按下 90ms（指尖到底就该看到结果），松手回到常量里那份 200ms 退场曲线，
 * 与底色进出同一套「进快出慢」的语言。
 * 缩放挂在 transform 上，所以调用方自己不许再写 translate/scale 类工具名——同一个 transform 属性，
 * 后写的会整条覆盖掉这份压缩曲线（Tailwind 的 transform 由自定义属性合成，但 duration/ease 仍是同类）。
 */
export const HOVER_PRESS = [
    "transition-[background-color,color,border-color,opacity,transform]",
    "duration-200 ease-[cubic-bezier(0.4,0,0.6,1)]",
    "hover:duration-180 hover:ease-[cubic-bezier(0.33,1,0.68,1)]",
    "active:scale-[0.98] active:duration-90 active:ease-out",
].join(" ");
