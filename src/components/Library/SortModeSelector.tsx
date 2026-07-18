import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useSettingsStore } from "@/stores/settings-store";
import type { LibrarySortMode } from "@/lib/song-sort";

const MODE_VALUES: LibrarySortMode[] = [
  "recently_imported",
  "title_asc",
  "artist_asc",
];

export function SortModeSelector() {
  const { t } = useTranslation();
  const librarySortMode = useSettingsStore((s) => s.librarySortMode);
  const setLibrarySortMode = useSettingsStore((s) => s.setLibrarySortMode);
  const [isPending, setIsPending] = useState(false);

  const handleChange = async (event: React.ChangeEvent<HTMLSelectElement>) => {
    const mode = event.target.value as LibrarySortMode;
    if (mode === librarySortMode) return;
    setIsPending(true);
    try {
      await setLibrarySortMode(mode);
    } finally {
      setIsPending(false);
    }
  };

  return (
    <select
      value={librarySortMode}
      onChange={handleChange}
      disabled={isPending}
      aria-label={t("sidebar.sortMode.label")}
      data-testid="sort-mode-selector"
      className="rounded-[8px] border border-[var(--sidebar-control-border)] bg-[var(--sidebar-control-bg)] px-1.5 py-0.5 text-[11px] text-[var(--color-text)] focus:border-[var(--color-control-primary)] focus:outline-none disabled:opacity-50"
    >
      {MODE_VALUES.map((mode) => (
        <option key={mode} value={mode}>
          {t(`sidebar.sortMode.${mode}`)}
        </option>
      ))}
    </select>
  );
}
