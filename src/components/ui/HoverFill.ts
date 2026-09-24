/**
 * 悬浮填充动效 HOVER_FILL —— 「悬浮选中态」灰色底（surface-2 一类）的出现/消失节奏
 *
 * 这里动的是**本来就存在的底色反馈**（hover:bg-*），不是外加投影。
 * 全应用所有带悬浮底色的控件统一挂这个常量，改观感只改这一处。
 *
 * 为什么留在 CSS 而不用 motion 驱动：
 *  - 底色由主题令牌（--surface-2 等）给出，hover:/条件类在 CSS 里换色；motion 驱动颜色的话
 *    要么把令牌色值抄成 JS 字面量（主题翻面即失真），要么给每个控件再加一层覆盖 DOM——
 *    后者正是上一版 Glow 投影层的成本与风险，而观感收益为零；
 *  - opacity 合成动画的优势（Dropzone 柔光层）只在「动画一个自带覆盖层的发光件」时成立，
 *    底色填充没有新增图层，CSS transition 就是最短路径。
 *
 * 时序：出现 180ms 快起慢收（ease-out），移开 200ms 慢起快出（ease-in）——
 * 进出都落在"感觉得到但等不了"以下，和 Tip 气泡的 120ms 淡出同一节奏语言。
 * 非 hover 态的那份参数就是**退场**曲线（悬停离开后元素回到无 hover: 前缀的状态）。
 *
 * 覆盖属性：background-color / color / border-color / opacity——比 transition-colors 多一个
 * opacity（primary 的 hover:opacity-90 也在做悬浮反馈，一并给节奏）。
 * 例外：Dropzone 拖放卡不挂本常量——它自带 `transition-[background-color,color,border-color,opacity]
 * duration-200`（描边透明↔accent 的激活态过渡），那 200ms 与卡片上浮弹簧是调校过的一体，不参与统一。
 * busy 的 opacity-70 也走同一条过渡（原 `transition-colors` 不含 opacity，会当场硬切）。
 * 副作用备案：主题切换瞬间被过渡的也就是这几个属性，若自测发现圆形波结束后有零星控件
 * "慢半拍换色"，把时长压回 150ms 即可（或届时再议冻结）。
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
