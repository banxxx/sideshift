/**
 * 模板表的读写（列表页与编辑页共用一份口径）
 *
 * 写只有一条路：整张表交回后端（`api.saveTemplates`）。逐条增删会做出「排序落了一半」的中间态，
 * 而这里一次改动最多涉及两条（换位/复制），不值得为它设计一套增量协议。
 * 乐观更新 + 失败回滚：先改界面再落盘，落盘失败把交回前那一份放回去并说清楚——静默回退最查不出来。
 * 两页各自抄一份 commit 的话，「保存失败」这句提示的口径迟早会分叉。
 */
import { useCallback, useEffect, useState } from "react";
import * as api from "@/lib/api";
import { notify } from "@/lib/notify";
import { errOf } from "@/lib/errors";
import { t } from "@/lib/i18n";
import type { ConversionTemplate } from "@/lib/types";

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
