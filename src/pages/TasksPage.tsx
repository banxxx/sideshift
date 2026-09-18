import { TaskList } from "@/components/features/TaskList";

export function TasksPage() {
    return (
        <div className="max-w-3xl mx-auto space-y-6">
            <div>
                <h1 className="text-2xl font-semibold tracking-tight">任务列表</h1>
                <p className="text-sm text-muted-foreground mt-1">
                    查看所有解析与构建任务的历史记录。
                </p>
            </div>
            <TaskList />
        </div>
    );
}