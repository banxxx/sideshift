/**
 * 切语言时把**视图**换 key 重挂。
 *
 * react-i18next 只重渲染**调过 `useTranslation` 的组件**，而这个仓库里有一批纯函数模块
 * （`lib/rail-view.ts` 的档位标签、`lib/format.ts` 的单位、各处 `label` 常量）在渲染期
 * 直接调 `t`——那些组件漏订一次就是「切了语言半页没换」。与其去几十个文件里赌没人漏，
 * 这里换 key 强制重挂，一条兜底盖住全部。
 *
 * 挂载位置在 `App` 的两个 Provider **之内**（见 App.tsx）：导航栈与包草稿是状态，
 * 不该跟着重挂——挂在外头时切完语言人会被弹回首页。
 *
 * 代价：切语言那一刻丢**组件内**的瞬时状态（开着的弹窗会关）。滚动位置是例外——
 * 页面级容器换 key 后会被 `lib/page-scroll.ts` 那条传送带贴回原位。
 * 这仍然可以接受，因为它只发生在设置页那一次操作上，而草稿、任务、设置缓存都住在模块级 store 里，
 * 不随挂载丢。`display:contents` 的壳不参与布局，包一层不会改变现有的 flex/grid 结构。
 */
import { useEffect, useState, type ReactNode } from "react";
import { stashScroll } from "@/lib/page-scroll";
import { activeLocale, onLanguageChanged } from "./index";

export function LocaleGate({ children }: { children: ReactNode }) {
    const [lng, setLng] = useState(activeLocale());
    useEffect(
        () =>
            onLanguageChanged(() => {
                // 换 key 之前先接走滚动位置：旧节点一没就读不到了（见 lib/page-scroll.ts）
                stashScroll();
                setLng(activeLocale());
            }),
        []
    );
    return (
        <div key={lng} className="contents">
            {children}
        </div>
    );
}
