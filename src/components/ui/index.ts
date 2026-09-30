/**
 * SideShift 设计规范公共原语出口（数值全部取自 SS.pen 各帧 dump，见 .design-ref/dump/*.txt）。
 * 所有页面一律用本目录组件拼装、统一从 "@/components/ui" 引，避免各页手写 class 导致跨屏漂移；不必知道某个件在哪个文件。
 * 提示口径：全应用不出现系统原生 title 气泡——Btn/IconBtn/ListRow 已在内部把 title 换成 aria-label + Tip，裸控件自己挂 TIP_TRIGGER 再放 <Tip/>。
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
/** 只读灰化那两套 class：自造控件（模板编辑页的目录格）要与真控件压得一模一样，口径得能引出去 */
export { READONLY_BOX, READONLY_MARK } from "./Field";
export { SearchSelect, type SelectOption } from "./SearchSelect";
export { SegTabs } from "./Tabs";
export { InlineRow, SectionTitle, SettingRow, NoteRow, ListRow } from "./Row";
export { Bar, CountRow, ChangeRow, InfoRow, MetaCell, MiniMeta } from "./Meta";
export { ModalShell } from "./Modal";
export { Swap } from "./Swap";
export { Collapse } from "./Collapse";
export { FoldBtn } from "./FoldBtn";
export { Tip, TIP_TRIGGER } from "./Tip";
export { HOVER_FILL, HOVER_PRESS } from "./HoverFill";
export { Logo } from "./Logo";
