import { PlayControls } from "./PlayControls";
import { SeekBar } from "./SeekBar";
import { VolumeSliders } from "./VolumeSliders";

export function PlaybackBar() {
  return (
    <div className="flex h-20 shrink-0 flex-col justify-center border-t border-[var(--color-border)] bg-[var(--color-toolbar)] px-6">
      <div className="mx-auto flex w-full max-w-4xl items-center gap-8">
        <PlayControls />
        <SeekBar />
        <VolumeSliders />
      </div>
    </div>
  );
}
