interface SettingsOverlayProps {
  onClose: () => void;
}

export function SettingsOverlay({ onClose }: SettingsOverlayProps) {
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
          <div className="space-y-3">
            <label className="text-[12px] font-medium uppercase text-[var(--color-text-dim)]">
              AI Separation Engine
            </label>
            <select className="w-full rounded-md border border-[var(--color-border-light)] bg-[var(--color-surface)] px-2 py-1.5 text-[13px] text-white focus:border-[var(--color-accent)] focus:outline-none">
              <option>OpenKara Core ML (Apple Silicon)</option>
              <option>OpenKara Fast (CPU)</option>
            </select>
          </div>
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
