import { PackUploader } from "@/components/features/PackUploader";

export function HomePage() {
    return (
        <div className="max-w-3xl mx-auto space-y-6">
            <div>
                <h1 className="text-2xl font-semibold tracking-tight">
                    整合包转服务端
                </h1>
                <p className="text-sm text-muted-foreground mt-1">
                    上传客户端整合包，自动生成可运行的服务端文件。
                </p>
            </div>
            <PackUploader />
        </div>
    );
}