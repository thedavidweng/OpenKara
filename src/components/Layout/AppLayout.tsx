import { useState } from "react";
import { Sidebar } from "./Sidebar";
import { Toolbar } from "./Toolbar";
import { PlaybackBar } from "@/components/Player/PlaybackBar";
import { LyricsPanel } from "@/components/Lyrics/LyricsPanel";
import { SettingsOverlay } from "@/components/Settings/SettingsOverlay";
import { ModelBootstrapBanner } from "@/components/Bootstrap/ModelBootstrapBanner";

export function AppLayout() {
  const [sidebarVisible, setSidebarVisible] = useState(true);
  const [settingsOpen, setSettingsOpen] = useState(false);

  return (
    <div className="flex h-screen w-full overflow-hidden">
      {sidebarVisible && <Sidebar />}

      <div className="flex flex-1 flex-col bg-[var(--color-surface)]">
        <Toolbar
          onToggleSidebar={() => setSidebarVisible(!sidebarVisible)}
          onToggleSettings={() => setSettingsOpen(!settingsOpen)}
          settingsOpen={settingsOpen}
        />

        <div className="relative flex flex-1 flex-col overflow-hidden">
          {settingsOpen && (
            <SettingsOverlay onClose={() => setSettingsOpen(false)} />
          )}
          <ModelBootstrapBanner />
          <LyricsPanel />
        </div>

        <PlaybackBar />
      </div>
    </div>
  );
}
