import { usePlayerStore } from "@/stores/player-store";
import type { LyricLine as LyricLineType } from "@/types/ipc";

interface LyricLineProps {
  line: LyricLineType;
  state: "active" | "past" | "future";
}

export function LyricLine({ line, state }: LyricLineProps) {
  const seek = usePlayerStore((s) => s.seek);

  const handleClick = () => {
    seek(line.time_ms);
  };

  return (
    <div
      onClick={handleClick}
      className={`flex cursor-pointer flex-col items-center gap-1.5 text-center transition-all duration-300 ${
        state === "active" ? "scale-105 drop-shadow-md" : ""
      }`}
    >
      <span
        className={`text-2xl font-bold tracking-tight transition-colors md:text-3xl ${
          state === "active"
            ? "text-white"
            : state === "past"
              ? "text-[var(--color-text-dimmer)]"
              : "text-[var(--color-active)]"
        }`}
      >
        {line.text}
      </span>
    </div>
  );
}
