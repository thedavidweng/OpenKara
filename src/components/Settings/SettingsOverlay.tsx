import { useState, useEffect } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { FolderOpen, Plus } from "lucide-react";
import * as api from "@/lib/tauri";

interface SettingsOverlayProps {
  onClose: () => void;
}

export function SettingsOverlay({ onClose }: SettingsOverlayProps) {
  const [libraryPath, setLibraryPath] = useState<string | null>(null);
  const [libraryError, setLibraryError] = useState<string | null>(null);

  useEffect(() => {
    api.getLibraryPath().then(setLibraryPath).catch(() => {});
  }, []);

  const handleCreateLibrary = async () => {
    const selected = await open({
      directory: true,
      title: "Choose a location for your Karaoke Library",
    });
    if (!selected) return;

    setLibraryError(null);
    try {
      await api.createLibrary(selected);
      setLibraryPath(selected);
    } catch (err: unknown) {
      setLibraryError(err instanceof Error ? err.message : String(err));
    }
  };

  const handleOpenLibrary = async () => {
    const selected = await open({
      directory: true,
      title: "Open an existing Karaoke Library",
    });
    if (!selected) return;

    setLibraryError(null);
    try {
      await api.openLibrary(selected);
      setLibraryPath(selected);
    } catch (err: unknown) {
      setLibraryError(err instanceof Error ? err.message : String(err));
    }
  };

  return (
    <div className="absolute inset-0 z-50 overflow-y-auto bg-[var(--color-surface)]/95 p-10 backdrop-blur-md">
      <div className="mx-auto max-w-xl overflow-hidden rounded-lg border border-[var(--color-border)] bg-[var(--color-sidebar)] shadow-2xl">
        <div className="flex items-center justify-between border-b border-[var(--color-border)] bg-[var(--color-hover)]/50 px-6 py-4">
          <span className="font-semibold text-white">Preferences</span>
          <button
            onClick={onClose}
            className="text-[12px] text-[var(--color-text-dim)] hover:text-white"
          >
            Close
          </button>
        </div>
        <div className="space-y-6 p-6">
          {/* Library Section */}
          <div className="space-y-3">
            <label className="text-[12px] font-medium uppercase text-[var(--color-text-dim)]">
              Karaoke Library
            </label>
            {libraryPath ? (
              <div className="rounded-md border border-[var(--color-border-light)] bg-[var(--color-surface)] px-3 py-2">
                <p className="truncate text-[13px] text-white" title={libraryPath}>
                  {libraryPath}
                </p>
              </div>
            ) : (
              <p className="text-[13px] text-[var(--color-text-dim)]">
                No library configured
              </p>
            )}
            <div className="flex gap-2">
              <button
                onClick={handleCreateLibrary}
                className="flex items-center gap-1.5 rounded-md border border-[var(--color-border-light)] bg-[var(--color-surface)] px-3 py-1.5 text-[12px] text-[var(--color-text)] transition-colors hover:bg-[var(--color-hover)] hover:text-white"
              >
                <Plus size={12} /> New Library
              </button>
              <button
                onClick={handleOpenLibrary}
                className="flex items-center gap-1.5 rounded-md border border-[var(--color-border-light)] bg-[var(--color-surface)] px-3 py-1.5 text-[12px] text-[var(--color-text)] transition-colors hover:bg-[var(--color-hover)] hover:text-white"
              >
                <FolderOpen size={12} /> Open Library
              </button>
            </div>
            {libraryError && (
              <p className="text-[12px] text-red-400">{libraryError}</p>
            )}
          </div>

          {/* AI Separation Engine */}
          <div className="space-y-3">
            <label className="text-[12px] font-medium uppercase text-[var(--color-text-dim)]">
              AI Separation Engine
            </label>
            <select className="w-full rounded-md border border-[var(--color-border-light)] bg-[var(--color-surface)] px-2 py-1.5 text-[13px] text-white focus:border-[var(--color-accent)] focus:outline-none">
              <option>OpenKara Core ML (Apple Silicon)</option>
              <option>OpenKara Fast (CPU)</option>
            </select>
          </div>

          {/* Output Device */}
          <div className="space-y-3">
            <label className="text-[12px] font-medium uppercase text-[var(--color-text-dim)]">
              Output Device
            </label>
            <select className="w-full rounded-md border border-[var(--color-border-light)] bg-[var(--color-surface)] px-2 py-1.5 text-[13px] text-white focus:border-[var(--color-accent)] focus:outline-none">
              <option>System Default</option>
            </select>
          </div>
        </div>
      </div>
    </div>
  );
}
