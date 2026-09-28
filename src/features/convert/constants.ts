/** Convert 页的静态配置与纯函数：下拉选项、行落位节拍、目录树定位 */
import { t } from "@/lib/i18n";
import type { SelectOption } from "@/components/ui";
import type { PackDirNode, PackDirTree, PackFileNode, VersionOption } from "@/lib/types";

/** VersionOption → 下拉项（group/recommended 透传，供分组与「推荐」标记） */
export const toOption = (v: VersionOption): SelectOption => ({
    value: v.value,
    label: v.label,
    recommended: v.recommended,
    group: v.group,
});

/** 按相对路径（kubejs/client_scripts）在目录树中定位节点，取递归文件数；查不到返回 undefined */
export function findDirNode(nodes: PackDirNode[], path: string): PackDirNode | undefined {
    const [head, ...rest] = path.split("/");
    const n = nodes.find((x) => x.name.toLowerCase() === head.toLowerCase());
    if (!n || rest.length === 0) return n;
    return n.children.length ? findDirNode(n.children, rest.join("/")) : undefined;
}

/** 按逻辑相对路径定位勾选中的文件：包根散文件在 `tree.files`，深层的挂在所在节点的 `files` 上。
 *  卡片行内要拿它读大小，只查 `tree.files` 的老写法对深层那档一律读不到 */
export function findFileNode(tree: PackDirTree, path: string): PackFileNode | undefined {
    const i = path.lastIndexOf("/");
    const want = (i < 0 ? path : path.slice(i + 1)).toLowerCase();
    if (i < 0) return tree.files.find((f) => f.name.toLowerCase() === want);
    return findDirNode(tree.dirs, path.slice(0, i))?.files.find((f) => f.name.toLowerCase() === want);
}

/** 卡片内直接展示的行数：卡高 280（内容 240 = p-5 后）− 头 36 − 距 14 = 190 给行区。
 *  行区内部按「行 5×20 + 距 4×10 + 间 10 + 出口（18~32）」≤ 182 排布，出口钉在行区底，
 *  余量只落在列表与出口之间；依赖警告出现时压缩行区（仅行可收缩裁切），
 *  其余走「查看全部」弹窗 */
export const PREVIEW_ROWS = 5;

/** 入场节拍：一批里每行错开 110ms 从右侧插入，一档一行。
 *  档数就取预览行数（卡内最多 5 行，排满即覆盖整个可见区，多余的行等「查看全部」弹窗） */
export const LAND_STAGGER_MS = 110;
export const LAND_MAX_STEPS = PREVIEW_ROWS - 1;

/** 页面入场节拍（容器 PAGE_RISE + 各块 CARD_RISE）已上收到 src/lib/page-motion.ts，全站共用 */

/** 游戏模式下拉项。`value` 是写进 server.properties / 发给后端的档位值，不翻；
 *  表建在函数体里（每格一条 `t(字面量)`）：顶层建表会把标签冻在首次加载的语言上，
 *  且「表里存中文再查表」那种写法自检认不出键 */
export function gamemodeOptions(): SelectOption[] {
    return [
        { value: "survival", label: t("convert.survival", "生存") },
        { value: "creative", label: t("convert.creative", "创造") },
        { value: "adventure", label: t("convert.adventure", "冒险") },
        { value: "spectator", label: t("convert.spectator", "旁观") },
    ];
}

/** 难度下拉项（同 gamemodeOptions：档位值不翻，表在函数体里现建） */
export function difficultyOptions(): SelectOption[] {
    return [
        { value: "peaceful", label: t("convert.peaceful", "和平") },
        { value: "easy", label: t("convert.easy", "简单") },
        { value: "normal", label: t("convert.normal", "普通") },
        { value: "hard", label: t("convert.hard", "困难") },
    ];
}
