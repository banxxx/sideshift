import { Card, CardContent } from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import { Input } from "@/components/ui/input";
import { Separator } from "@/components/ui/separator";

export function SettingsPage() {
    return (
        <div className="max-w-3xl mx-auto space-y-6">
            <div>
                <h1 className="text-2xl font-semibold tracking-tight">设置</h1>
                <p className="text-sm text-muted-foreground mt-1">
                    配置下载并发数、输出目录等选项。
                </p>
            </div>

            <Card>
                <CardContent className="p-6 space-y-6">
                    <div className="space-y-2">
                        <Label htmlFor="concurrency">下载并发数</Label>
                        <Input id="concurrency" type="number" defaultValue={8} />
                    </div>
                    <Separator />
                    <div className="space-y-2">
                        <Label htmlFor="output">服务端输出目录</Label>
                        <Input id="output" placeholder="留空则每次询问" />
                    </div>
                </CardContent>
            </Card>
        </div>
    );
}