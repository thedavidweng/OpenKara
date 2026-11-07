import { useCallback } from "react";
import { useTranslation } from "react-i18next";
import { Globe, Search } from "lucide-react";
import { sourceReady, useCatalogStore } from "@/stores/catalog-store";
import { useLibraryStore } from "@/stores/library-store";

export function SearchBox() {
  const { t } = useTranslation();
  const searchQuery = useLibraryStore((s) => s.searchQuery);
  const setSearchQuery = useLibraryStore((s) => s.setSearchQuery);
  const searchEverywhere = useCatalogStore((s) => s.searchEverywhere);
  const canSearchSources = useCatalogStore((s) =>
    s.sources.some(
      (source) =>
        sourceReady(source) && source.manifest?.capabilities.includes("search"),
    ),
  );
  const query = searchQuery.trim();
  const offerSources = canSearchSources && query !== "";

  const handleChange = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      setSearchQuery(e.target.value);
    },
    [setSearchQuery],
  );

  return (
    <div className="space-y-1">
      <div
        className="motion-surface relative flex items-center overflow-hidden rounded-[14px] border border-[var(--sidebar-control-border)] bg-[var(--sidebar-control-bg)] shadow-[inset_0_1px_0_var(--sidebar-control-border)] focus-within:border-[color-mix(in_srgb,var(--color-accent)_24%,var(--color-border-light))] focus-within:bg-[var(--sidebar-row-overlay-bg)]"
        data-search-visual-variant="unified"
      >
        <Search
          className="absolute left-3 text-[var(--color-text-dim)]"
          size={14}
        />
        <input
          type="text"
          placeholder={t("common.search")}
          aria-label={t("common.search")}
          aria-keyshortcuts={offerSources ? "Enter" : undefined}
          value={searchQuery}
          onChange={handleChange}
          onKeyDown={(event) => {
            if (
              offerSources &&
              event.key === "Enter" &&
              !event.nativeEvent.isComposing
            ) {
              event.preventDefault();
              void searchEverywhere(query);
            }
          }}
          className="w-full bg-transparent py-2 pl-9 pr-3 text-[14px] text-[var(--color-text)] outline-none placeholder:text-[var(--color-text-dim)]"
        />
      </div>
      {offerSources ? (
        <button
          type="button"
          onClick={() => void searchEverywhere(query)}
          className="sidebar-source-list-row motion-surface flex min-h-7 w-full items-center gap-2 border border-transparent px-2 py-1 text-left text-[12px] text-[var(--color-text)] hover:bg-[var(--sidebar-row-overlay-bg)] focus-visible:outline-2 focus-visible:outline-[var(--color-focus-ring)]"
        >
          <Globe
            size={13}
            aria-hidden="true"
            className="shrink-0 text-[var(--color-accent)]"
          />
          <span className="truncate">
            {t("globalSearch.searchSources", { query })}
          </span>
        </button>
      ) : null}
    </div>
  );
}
