import { useState } from "react";
import { X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useRotationStore } from "@/stores/rotation-store";

interface AddSingerInputProps {
  onAdd: (name: string) => void;
}

function AddSingerInput({ onAdd }: AddSingerInputProps) {
  const [open, setOpen] = useState(false);
  const [value, setValue] = useState("");

  const handleSubmit = () => {
    if (value.trim()) {
      onAdd(value.trim());
      setValue("");
      setOpen(false);
    }
  };

  if (!open) {
    return (
      <button
        type="button"
        onClick={() => setOpen(true)}
        className="flex items-center gap-1 rounded border border-dashed border-[var(--color-border)] px-2 py-0.5 text-[11px] text-[var(--color-text-dim)] transition-colors hover:border-[var(--color-accent)] hover:text-[var(--color-accent)]"
      >
        + Add Singer
      </button>
    );
  }

  return (
    <input
      autoFocus
      value={value}
      onChange={(e) => setValue(e.target.value)}
      onKeyDown={(e) => {
        if (e.key === "Enter") handleSubmit();
        if (e.key === "Escape") {
          setValue("");
          setOpen(false);
        }
      }}
      onBlur={() => {
        if (value.trim()) handleSubmit();
        else {
          setValue("");
          setOpen(false);
        }
      }}
      placeholder="Singer name"
      className="min-w-[80px] rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-0.5 text-[11px] text-[var(--color-text)] outline-none focus:border-[var(--color-accent)]"
    />
  );
}

interface SingerTagProps {
  name: string;
  isCurrent: boolean;
  onSelect: () => void;
  onRemove: () => void;
}

function SingerTag({ name, isCurrent, onSelect, onRemove }: SingerTagProps) {
  return (
    <span
      className={`flex items-center overflow-hidden rounded-full text-[11px] ${
        isCurrent
          ? "bg-[var(--color-accent)] text-white"
          : "bg-[var(--color-hover)] text-[var(--color-text)]"
      }`}
    >
      <button
        type="button"
        onClick={onSelect}
        className={`px-2 py-0.5 text-left ${isCurrent ? "font-medium" : ""}`}
        aria-pressed={isCurrent}
      >
        {name}
      </button>
      <button
        type="button"
        onClick={onRemove}
        className="mr-1 flex items-center rounded-full p-0.5 hover:opacity-70"
        aria-label={`Remove ${name}`}
      >
        <X size={10} />
      </button>
    </span>
  );
}

export function RotationControls() {
  const { t } = useTranslation();
  const {
    active,
    singerNames,
    currentIndex,
    toggleActive,
    addSinger,
    removeSinger,
    advanceRotation,
    setCurrentSinger,
  } = useRotationStore();

  if (!active && singerNames.length === 0) {
    return (
      <div className="flex items-center justify-between border-b border-[color-mix(in_srgb,var(--color-border)_86%,transparent)] px-4 py-2">
        <span className="text-[12px] font-medium text-[var(--color-control-primary)]">
          {t("rotation.singer")}
        </span>
        <button
          type="button"
          onClick={toggleActive}
          className="motion-icon-button rounded px-1.5 py-1 text-[11px] text-[var(--color-text-dimmer)] hover:bg-[var(--color-ghost-hover)] hover:text-[var(--color-control-primary)]"
        >
          {t("rotation.roundRobin")}
        </button>
      </div>
    );
  }

  return (
    <div className="border-b border-[color-mix(in_srgb,var(--color-border)_86%,transparent)] px-3 py-2 space-y-2">
      <div className="flex items-center justify-between">
        <span className="text-[11px] font-semibold text-[var(--color-text-dim)]">
          {t("rotation.singer")}
        </span>
        <button
          type="button"
          onClick={toggleActive}
          className={`motion-icon-button rounded px-1.5 py-0.5 text-[11px] transition-colors ${
            active
              ? "bg-[var(--color-accent)] text-white"
              : "text-[var(--color-text-dimmer)] hover:bg-[var(--color-ghost-hover)] hover:text-[var(--color-control-primary)]"
          }`}
        >
          {active ? t("rotation.roundRobin") : t("rotation.singer")}
        </button>
      </div>

      {active && (
        <div className="flex flex-wrap items-center gap-1">
          {singerNames.map((name, i) => (
            <SingerTag
              key={name}
              name={name}
              isCurrent={i === currentIndex}
              onSelect={() => void setCurrentSinger(name)}
              onRemove={() => void removeSinger(name)}
            />
          ))}
          <AddSingerInput onAdd={addSinger} />
          <button
            type="button"
            onClick={() => void advanceRotation()}
            disabled={singerNames.length === 0}
            className="ml-auto rounded border border-[var(--color-border)] px-2 py-0.5 text-[11px] text-[var(--color-text-dim)] transition-colors hover:border-[var(--color-accent)] hover:text-[var(--color-accent)] disabled:opacity-40"
          >
            {t("rotation.nextSinger")}
          </button>
        </div>
      )}
    </div>
  );
}
