/**
 * 「发现新版本」那扇窗的挂载点：挂在 Shell 而不是设置页——换页会把整棵页面子树卸掉，
 * 而取件那一轮（下载 / 校验）是进程级的：人去首页转一圈，回来该接着看同一扇窗。
 *
 * 这一层单独存在是为了**吃订阅**：进度事件每跳一次 store 就换一次引用，Shell 直接读它等于
 * 每跳重画整棵应用（标题栏、侧栏、当前页全跟着走）。订阅收在这片叶子上，别的层只读它自己的档位
 */
import { UpdateDialog } from "@/features/update/UpdateDialog";
import { cancelFetch, closeUpdate, installUpdate, startFetch, useUpdate } from "@/lib/update-store";

export function UpdateLayer() {
    const { open, info, status } = useUpdate();
    return (
        <UpdateDialog
            info={open ? info : null}
            status={status}
            onClose={closeUpdate}
            onStart={startFetch}
            onCancel={cancelFetch}
            onInstall={installUpdate}
        />
    );
}
