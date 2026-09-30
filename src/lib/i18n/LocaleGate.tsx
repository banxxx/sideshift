/**
 * 切语言时把**视图**换 key 整棵重挂，兜住漏订语言的组件（react-i18next 只重渲染订过 `useTranslation` 的）。
 * 挂载位置必须在 `App` 两个 Provider **之内**：导航栈与包草稿是状态，挂在外头切完语言人会被弹回首页。
 * 代价：切语言那一刻丢组件内瞬时状态；滚动位置由 `lib/page-scroll.ts` 贴回原位。
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
