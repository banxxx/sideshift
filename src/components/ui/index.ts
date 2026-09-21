/**
 * SideShift 设计规范公共原语（数值全部取自 SS.pen 各帧 dump，见 .design-ref/dump/*.txt）
 *
 * 标尺速查：
 *  - 卡片：$surface + $stroke 1px + r12 + padding 20；纵向 gap 按帧不同 → Panel 的 gap 属性传入。
 *  - 卡内标题：13/600 $text-1；设置行标题：13/600 $text-1 + 11 $text-3 说明。
 *  - 控件高度：主按钮 36、次按钮/输入/选择/开关轨 32(开关本体 20)、分段轨 36（内项 30）。
 *  - 字号阶梯 10/11/12/13/14/16/22；徽章 r99，小元素 r6，控件 r8。
 * 所有页面一律用本目录组件拼装，避免各页手写 class 导致跨屏漂移。
 *
 * 分文件按职责走：Panel 骨架 / Chip 色调芯片 / Button 按钮 / Field 控件 /
 * SearchSelect 下拉 / Tabs 分段 / Row 行排版 / Meta 计数与元数据 / Modal 弹窗壳 / Tip 悬停气泡。
 * 页面统一从 "@/components/ui" 引，不必知道某个件在哪个文件里。
 *
 * 提示口径：全应用不出现系统原生 title 气泡——Btn/IconBtn/ListRow 已在内部把 title
 * 换成 aria-label + Tip，裸控件则自己挂 TIP_TRIGGER 再放一个 <Tip/>。
 */
export {
    PageHeader,
    Panel,
    PanelHead,
    Divider,
} from "./Panel";
export { ToneChip, TagChip, type Tone } from "./Chip";
export { Btn, IconBtn, LinkBtn } from "./Button";
export { Stepper, Toggle, TextInput, CheckBox, SearchBox } from "./Field";
export { SearchSelect, type SelectOption } from "./SearchSelect";
export { SegTabs, SEG_PILL_SPRING } from "./Tabs";
export { InlineRow, SectionTitle, SettingRow, NoteRow, ListRow } from "./Row";
export { Bar, CountRow, ChangeRow, InfoRow, MetaCell, MiniMeta } from "./Meta";
export { ModalShell } from "./Modal";
export { Tip, TIP_TRIGGER } from "./Tip";
