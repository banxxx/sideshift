/**
 * 模板表的读写（列表页与编辑页共用一份口径）：写只有一条路——整张表交回后端（`api.saveTemplates`），避免「排序落了一半」的中间态。
 * 乐观更新 + 失败回滚（落盘失败放回原数据并提示，不静默回退）；两页共用同一份 commit，报错口径不分叉。
 */
import { useCallback, useEffect, useState } from "react";
import * as api from "@/lib/api";
import { notify } from "@/lib/notify";
import { errOf } from "@/lib/errors";
import { t } from "@/lib/i18n";
import { TEMPLATE_MAX, type ConversionTemplate } from "@/lib/types";

/** 屏上这一档计数是否已经到顶（200 份）：只有「再建一份」的入口问它 */
const atCap = (count: number) => count >= TEMPLATE_MAX;

/** 拒讯只写这一份：入口那道闸门和写盘那道共用它，口径不会分叉 */
function reportCap() {
    notify(
        t("templates.cap-reached", "模板已达 {{max}} 份上限 · 先删掉一份才能再建", {
            max: TEMPLATE_MAX,
        }),
        "error"
    );
}

/**
 * 三个「再去建一份」的入口（列表页页头、转换页小卡的新建/存为、编辑页另存为副本）共用的前置闸门。
 * 放在跳转之前而不是只放在写盘处：让人填完整张表单、勾完十六行、点保存才被告知「建不了」，
 * 是最亏的一趟。写盘那道（`commit`）照旧存在，这一道只是把拒讯提前。
 */
export function guardTemplateCap(count: number): boolean {
    if (!atCap(count)) return true;
    reportCap();
    return false;
}

export function useTemplateTable() {
    // 首帧吃上一趟读数（同任务列表页）：换页会重挂载，空着挂载就是占位层叠在真内容上换一次
    const [templates, setTemplates] = useState<ConversionTemplate[]>(() => api.peekTemplates() ?? []);
    /** 首轮读数到手前不许宣布「还没有模板」、也不许铺编辑草稿：读盘是异步的，抢在前头会闪一张空壳 */
    const [loaded, setLoaded] = useState(() => api.peekTemplates() !== null);

    useEffect(() => {
        api
            .listTemplates()
            .then(setTemplates)
            // 读失败按空表处理，但照样算「读到了」：闸门一直压着会让页面卡在占位上，比报一次空更糟
            .catch(() => setTemplates([]))
            .finally(() => setLoaded(true));
    }, []);

    /**
     * 交回整张表；返回有没有落盘成功（编辑页靠它决定「留在这一页还是退回上一层」）。
     * 回滚的基准是 `api.peekTemplates()`（磁盘那一份），**不是本 render 的 `templates`**：拖动排序在写盘
     * 之前已经 `setTemplates` 过好几轮，闭包那份很可能停在「拖到一半」的顺序，拿它回滚就等于
     * 「保存失败后屏上排好了、磁盘没排」，下次进页又跳回去——正是最难查的那种。
     * `peek` 为 null 是这个进程还没碰过磁盘，此时没有可信基准，维持现状等下一次读数。
     */
    const commit = useCallback(async (next: ConversionTemplate[]): Promise<boolean> => {
        const prev = api.peekTemplates();
        /**
         * 数量闸门（200）：只挡「让表变长」的那一趟 ⇒ 编辑既有、删除、拖动排序都不受这一档牵连，
         * 已经超限的表（手改过磁盘文件的）照样能删回去。基准取磁盘那份而不是本闭包的 `templates`，
         * 与下面回滚同一个理由。
         */
        if (next.length > TEMPLATE_MAX && (!prev || next.length > prev.length)) {
            reportCap();
            return false;
        }
        setTemplates(next);
        try {
            await api.saveTemplates(next);
            return true;
        } catch (e) {
            if (prev) setTemplates(prev);
            notify(
                t("templates.save-failed", "模板保存失败：{{reason}}", { reason: errOf(e) }),
                "error"
            );
            return false;
        }
    }, []);

    return { templates, setTemplates, loaded, commit };
}
