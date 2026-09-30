/**
 * 转换模板的读写（Rust: list_templates / set_templates / template_defaults）。
 * 模板是有序列表（顺序即下拉顺序），一律**整表写回**，不做逐条增删改；新建模板的默认值只取 `template_defaults`，前端不抄第二份字面量。
 */
import type { ConversionTemplate, TemplateValues } from "@/lib/types";
import * as mock from "@/lib/mock";
import { invokeOrMock, isTauri } from "./client";

/**
 * 最近一份模板表（模块级，跨页面挂载存续）：列表页与转换页那张小卡都要它，
 * 没有它就得先画一帧空态再换成真内容（读盘是异步的，抢在前头会闪一张「还没有模板」）。
 * `saveTemplates` 成功也写这里，刚改过的那几条下一次进页不会闪回旧值。
 */
let lastTemplates: ConversionTemplate[] | null = null;

/** 上一份读到/存掉的模板表；null = 这个进程还没碰过模板（首帧仍走骨架/不画空态） */
export function peekTemplates(): ConversionTemplate[] | null {
    return lastTemplates;
}

/** 读整张模板表（已按用户的排序返回） */
export async function listTemplates(): Promise<ConversionTemplate[]> {
    const list = isTauri
        ? await invokeOrMock<ConversionTemplate[]>("list_templates", undefined, () =>
              mock.mockListTemplates()
          )
        : await mock.mockListTemplates();
    lastTemplates = list;
    return list;
}

/**
 * 写整张模板表（Rust: set_templates）。先落盘再改内存：写失败时内存仍是旧那张表，
 * 调用方据此回滚，不会出现「界面已经排好、重启又变回去」。
 */
export async function saveTemplates(list: ConversionTemplate[]): Promise<void> {
    if (!isTauri) {
        mock.mockSaveTemplates(list);
        lastTemplates = list;
        return;
    }
    await invokeOrMock<void>("set_templates", { templates: list }, () =>
        mock.mockSaveTemplates(list)
    );
    // 写在 await 之后：后端拒绝时这份必须还是磁盘上那一份
    lastTemplates = list;
}

/** 新建模板的种子值（16 档全在，勾哪档才写进模板） */
export async function templateDefaults(): Promise<TemplateValues> {
    if (!isTauri) return mock.mockTemplateDefaults();
    return invokeOrMock<TemplateValues>("template_defaults", undefined, () =>
        mock.mockTemplateDefaults()
    );
}

/** 模板 id：前端造（新建/复制都在前端），与任务 id 同一族前缀，好一眼认出是谁生的 */
export const newTemplateId = (): string =>
    `tpl-${crypto.randomUUID().replace(/-/g, "").slice(0, 12)}`;
