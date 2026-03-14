import { Music } from "lucide-react";
import { ImportButton } from "./ImportButton";

export function EmptyLibrary() {
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-3 px-4 py-8 text-center">
      <Music size={32} className="text-[var(--color-text-dimmer)]" />
      <p className="text-[12px] text-[var(--color-text-dim)]">No tracks yet</p>
      <ImportButton>
        <span className="rounded-md border border-[var(--color-border-light)] bg-[var(--color-hover)] px-3 py-1 text-[12px] text-[var(--color-text)] transition-colors hover:bg-[var(--color-active)] hover:text-white">
          Import Music
        </span>
      </ImportButton>
    </div>
  );
}
