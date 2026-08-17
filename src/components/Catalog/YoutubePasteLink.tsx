import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useBackend } from "@/lib/backend";
import { notifyError } from "@/lib/errors";
import { useCatalogStore } from "@/stores/catalog-store";
import { useQueueStore } from "@/stores/queue-store";
import { usePlayerStore } from "@/stores/player-store";

export function YoutubePasteLink() {
  const { catalog } = useBackend();
  const { t } = useTranslation();
  const [url, setUrl] = useState("");
  const rememberVideoItems = useCatalogStore((s) => s.rememberVideoItems);
  const addToQueue = useQueueStore((s) => s.addToQueue);
  const playNow = usePlayerStore((s) => s.playNow);

  const submit = async (play: boolean) => {
    const trimmed = url.trim();
    if (!trimmed) return;
    try {
      const items = await catalog.resolveVideoSourceUrl("youtube", trimmed);
      rememberVideoItems(items);
      if (items[0] && play) {
        await playNow(items[0].id);
        for (const item of items.slice(1)) {
          addToQueue(item.id);
        }
      } else {
        for (const item of items) {
          addToQueue(item.id);
        }
      }
      setUrl("");
    } catch (error) {
      notifyError(error);
    }
  };

  return (
    <form
      className="flex flex-col gap-2 px-3 py-2"
      onSubmit={(event) => {
        event.preventDefault();
        void submit(false);
      }}
    >
      <label className="text-[11px] font-semibold tracking-wide text-[var(--color-text-dim)]">
        {t("youtube.pasteLink")}
      </label>
      <input
        type="url"
        value={url}
        onChange={(event) => setUrl(event.target.value)}
        placeholder={t("youtube.pastePlaceholder")}
        aria-label={t("youtube.pasteLink")}
        className="w-full rounded-md border border-[var(--color-border-light)] bg-[var(--color-surface)] px-2 py-1.5 text-[13px] text-[var(--color-text)]"
      />
      <div className="flex gap-2">
        <button
          type="submit"
          className="rounded-md border border-[var(--color-border-light)] px-3 py-1.5 text-[12px] text-[var(--color-text)]"
        >
          {t("youtube.addToQueue")}
        </button>
        <button
          type="button"
          onClick={() => void submit(true)}
          className="rounded-md border border-[var(--color-border-light)] px-3 py-1.5 text-[12px] text-[var(--color-text)]"
        >
          {t("library.playNow")}
        </button>
      </div>
    </form>
  );
}
