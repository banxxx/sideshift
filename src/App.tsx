import { useState } from "react";
import { Sidebar, type PageKey } from "@/components/layout/Sidebar";
import { TitleBar } from "@/components/layout/TitleBar";
import { HomePage } from "@/pages/HomePage";
import { TasksPage } from "@/pages/TasksPage";
import { SettingsPage } from "@/pages/SettingsPage";
import "./App.css";

function App() {
    const [page, setPage] = useState<PageKey>("home");

    return (
        <div
            className="h-screen flex bg-background text-foreground"
            onContextMenu={
                import.meta.env.PROD ? (e) => e.preventDefault() : undefined
            }
        >
            <Sidebar current={page} onNavigate={setPage} />
            <div className="flex-1 flex flex-col min-w-0">
                <TitleBar />
                <main className="flex-1 overflow-auto p-6">
                    {page === "home" && <HomePage />}
                    {page === "tasks" && <TasksPage />}
                    {page === "settings" && <SettingsPage />}
                </main>
            </div>
        </div>
    );
}

export default App;