import { Card, CardContent } from "@/components/ui/card";
import { ListChecks } from "lucide-react";

export function TaskList() {
    // TODO: 后续接真实任务数据
    const tasks: { id: string; name: string; status: string }[] = [];

    if (tasks.length === 0) {
        return (
            <Card>
                <CardContent className="p-12 text-center">
                    <ListChecks className="h-12 w-12 mx-auto mb-4 text-muted-foreground" />
                    <p className="text-sm text-muted-foreground">暂无任务</p>
                </CardContent>
            </Card>
        );
    }

    return (
        <div className="space-y-3">
            {tasks.map((t) => (
                <Card key={t.id}>
                    <CardContent className="p-4">
                        <div className="font-medium">{t.name}</div>
                        <div className="text-xs text-muted-foreground">{t.status}</div>
                    </CardContent>
                </Card>
            ))}
        </div>
    );
}