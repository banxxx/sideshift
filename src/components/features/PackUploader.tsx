import { Upload, FileArchive } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { useState } from "react";

export function PackUploader() {
    const [fileName, setFileName] = useState<string | null>(null);

    const handlePick = () => {
        // TODO: 后续接 Tauri dialog + invoke
        setFileName("示例-整合包.zip");
    };

    return (
        <Card>
            <CardContent className="p-8">
                <div
                    className="border-2 border-dashed rounded-lg p-10 text-center cursor-pointer hover:border-primary/50 transition-colors"
                    onClick={handlePick}
                >
                    {fileName ? (
                        <>
                            <FileArchive className="h-12 w-12 mx-auto mb-4 text-primary" />
                            <p className="text-sm font-medium">{fileName}</p>
                            <p className="text-xs text-muted-foreground mt-1">
                                点击重新选择
                            </p>
                        </>
                    ) : (
                        <>
                            <Upload className="h-12 w-12 mx-auto mb-4 text-muted-foreground" />
                            <p className="text-sm font-medium">点击选择整合包文件</p>
                            <p className="text-xs text-muted-foreground mt-1">
                                支持 .zip / .7z / .mrpack 格式
                            </p>
                        </>
                    )}
                </div>

                <div className="mt-4 flex justify-end">
                    <Button disabled={!fileName}>开始解析</Button>
                </div>
            </CardContent>
        </Card>
    );
}