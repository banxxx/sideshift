/**
 * Convert 页弹窗族（SS.pen `PCRJi` 剔除清单 / `St8m8` 网络添加 / `wphzw` 模组详情）
 *
 * 定稿规则：
 *  - 遮罩 50% 黑；模态 $surface + $stroke 1px r12 padding20 gap12；页脚 = 1px $stroke + 摘要 + 按钮组
 *  - 网络添加与模组详情是同壳二级视图，尺寸强制一致 800×464（“跳转后弹窗不能变小”）
 *  - 处置清单壳剔除/保留/新增共用（focus 区分）：勾选位 = 是否进服务端包，
 *    与卡内 CheckBox 同方向（剔除窗默认全不勾、保留/新增窗默认全勾）；
 *    金色行 = 相对本清单处置被改判、还没应用；勾选只进弹窗草稿，
 *    「应用」时才把差异行回写页面（「取消」/关闭按钮放弃草稿）
 */
export { DirPickerModal } from "./DirPickerModal";
export { OnlineAddModal } from "./OnlineAddModal";
export { PlanListModal, type ListFocus } from "./PlanListModal";
export { SideChip } from "./SideChip";
