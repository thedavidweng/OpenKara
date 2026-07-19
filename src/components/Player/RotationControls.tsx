import { useCallback, useState } from "react";
import { Shuffle, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useRotationStore } from "@/stores/rotation-store";
import { useQueueStore } from "@/stores/queue-store";
import { ConfirmationDialog } from "@/components/Settings/ConfirmationDialog";

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
      aria-label="Singer name"
      className="min-w-[80px] rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-0.5 text-[11px] text-[var(--color-text)] outline-none focus:border-[var(--color-accent)]"
    />
  );
}

interface SingerTagProps {
  name: string;
  isSelected: boolean;
  onSelect: () => void;
  onRemove: () => void;
}

function SingerTag({ name, isSelected, onSelect, onRemove }: SingerTagProps) {
  return (
    <span
      className={`flex items-center overflow-hidden rounded-full text-[11px] ${
        isSelected
          ? "bg-[var(--color-accent)] text-[var(--color-on-accent)]"
          : "bg-[var(--color-hover)] text-[var(--color-text)]"
      }`}
    >
      <button
        type="button"
        onClick={onSelect}
        className={`px-2 py-0.5 text-left ${isSelected ? "font-medium" : ""}`}
        aria-pressed={isSelected}
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
    singerNames,
    filterSinger,
    queueSingers,
    addSinger,
    removeSinger,
    shuffleQueue,
    setFilterSinger,
  } = useRotationStore();
  const queue = useQueueStore((s) => s.queue);
  const removeFromQueue = useQueueStore((s) => s.removeFromQueue);
  const [confirmRemove, setConfirmRemove] = useState<string | null>(null);

  const handleRemoveSinger = useCallback(
    (name: string) => {
      const assignedCount = [...queueSingers.values()].filter(
        (s) => s === name,
      ).length;
      if (assignedCount === 0) {
        void removeSinger(name);
      } else {
        setConfirmRemove(name);
      }
    },
    [queueSingers, removeSinger],
  );

  const handleConfirmRemove = useCallback(() => {
    if (!confirmRemove) return;
    const indicesToRemove: number[] = [];
    queue.forEach((songId, index) => {
      if (queueSingers.get(songId) === confirmRemove) {
        indicesToRemove.push(index);
      }
    });
    for (let i = indicesToRemove.length - 1; i >= 0; i--) {
      removeFromQueue(indicesToRemove[i]);
    }
    void removeSinger(confirmRemove);
    if (filterSinger === confirmRemove) {
      setFilterSinger(null);
    }
    setConfirmRemove(null);
  }, [
    confirmRemove,
    queue,
    queueSingers,
    removeFromQueue,
    removeSinger,
    filterSinger,
    setFilterSinger,
  ]);

  return (
    <div className="border-b border-[color-mix(in_srgb,var(--color-border)_86%,transparent)] px-3 py-2 space-y-2">
      <div className="flex items-center justify-between">
        <span className="text-[11px] font-semibold text-[var(--color-text-dim)]">
          {t("rotation.singer")}
        </span>
        <button
          type="button"
          onClick={shuffleQueue}
          disabled={queue.length <= 1}
          className="motion-icon-button flex items-center gap-1 rounded px-2 py-0.5 text-[11px] text-[var(--color-text-dim)] transition-colors hover:bg-[var(--color-ghost-hover)] hover:text-[var(--color-control-primary)] disabled:opacity-30"
        >
          <Shuffle size={11} />
          {t("rotation.shuffle")}
        </button>
      </div>

      {singerNames.length > 0 && (
        <div className="flex flex-wrap items-center gap-1">
          {singerNames.map((name) => (
            <SingerTag
              key={name}
              name={name}
              isSelected={filterSinger === name}
              onSelect={() =>
                setFilterSinger(filterSinger === name ? null : name)
              }
              onRemove={() => handleRemoveSinger(name)}
            />
          ))}
          <AddSingerInput onAdd={addSinger} />
        </div>
      )}

      {singerNames.length === 0 && <AddSingerInput onAdd={addSinger} />}

      {confirmRemove && (
        <ConfirmationDialog
          title={t("rotation.confirmRemoveSinger")}
          message={t("rotation.confirmRemoveSingerMessage")}
          confirmLabel={t("rotation.removeSinger")}
          onConfirm={handleConfirmRemove}
          onCancel={() => setConfirmRemove(null)}
        />
      )}
    </div>
  );
}
