import { useEffect, useState } from "react";
import { Tooltip } from "@/components/Overlay/Tooltip";

interface AudioLevelSliderProps {
  label: string;
  value: number;
  onChange: (value: number) => void;
  onDragStart?: () => void;
  onDragEnd?: () => void;
  disabled?: boolean;
  widthClass?: string;
  ariaLabel?: string;
  inputRef?: React.RefObject<HTMLInputElement | null>;
}

function formatAudioLevelTooltip(label: string, value: number): string {
  return `${label} ${Math.round(value * 100)}%`;
}

export function AudioLevelSlider({
  label,
  value,
  onChange,
  onDragStart,
  onDragEnd,
  disabled = false,
  widthClass = "w-16",
  ariaLabel,
  inputRef,
}: AudioLevelSliderProps) {
  const [isDragging, setIsDragging] = useState(false);

  useEffect(() => {
    if (!isDragging) {
      return;
    }

    const handlePointerFinish = () => {
      setIsDragging(false);
      onDragEnd?.();
    };

    window.addEventListener("pointerup", handlePointerFinish);
    window.addEventListener("pointercancel", handlePointerFinish);

    return () => {
      window.removeEventListener("pointerup", handlePointerFinish);
      window.removeEventListener("pointercancel", handlePointerFinish);
    };
  }, [isDragging, onDragEnd]);

  return (
    <Tooltip label={formatAudioLevelTooltip(label, value)}>
      <input
        ref={inputRef}
        type="range"
        min="0"
        max="100"
        value={Math.round(value * 100)}
        onChange={(e) => onChange(Number(e.target.value) / 100)}
        onPointerDown={() => {
          setIsDragging(true);
          onDragStart?.();
        }}
        onBlur={() => {
          setIsDragging(false);
          onDragEnd?.();
        }}
        className={`native-slider audio-level-slider shrink-0 ${widthClass}`}
        disabled={disabled}
        data-dragging={isDragging ? "true" : undefined}
        aria-label={ariaLabel ?? label}
      />
    </Tooltip>
  );
}
