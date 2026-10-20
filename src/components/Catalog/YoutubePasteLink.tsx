import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useBackend } from "@/lib/backend";
import { notifyError } from "@/lib/errors";
import { useCatalogStore } from "@/stores/catalog-store";
import { useQueueStore } from "@/stores/queue-store";
import { usePlayerStore } from "@/stores/player-store";

const focusRing =
  "focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-focus-ring)]";

function isHttpUrl(value: string): boolean {
  try {
    const parsed = new URL(value);
    return parsed.protocol === "http:" || parsed.protocol === "https:";
  } catch {
    return false;
  }
}

export function YoutubePasteLink({
  onDone,
  inputRef,
}: {
  onDone?: () => void;
  inputRef?: React.RefObject<HTMLInputElement | null>;
}) {
  const { catalog } = useBackend();
  const { t } = useTranslation();
  const [url, setUrl] = useState("");
  const [invalid, setInvalid] = useState(false);
  const [busy, setBusy] = useState(false);
  const fallbackRef = useRef<HTMLInputElement>(null);
  const rememberVideoItems = useCatalogStore((s) => s.rememberVideoItems);
  const addToQueue = useQueueStore((s) => s.addToQueue);
  const playNow = usePlayerStore((s) => s.playNow);

  const submit = async (play: boolean) => {
    const trimmed = url.trim();
    if (!trimmed) return;
    if (!isHttpUrl(trimmed)) {
      setInvalid(true);
      return;
    }
    setInvalid(false);
    setBusy(true);
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
      onDone?.();
    } catch (error) {
      notifyError(error);
    } finally {
      setBusy(false);
    }
  };

  const buttonBase = `min-h-9 rounded-md px-3 text-[13px] disabled:opacity-50 ${focusRing}`;

  return (
    <form
      noValidate
      className="flex flex-col gap-3"
      onSubmit={(event) => {
        event.preventDefault();
        void submit(false);
      }}
    >
      <input
        ref={inputRef ?? fallbackRef}
        type="url"
        value={url}
        onChange={(event) => {
          setUrl(event.target.value);
          setInvalid(false);
        }}
        placeholder={t("youtube.pastePlaceholder")}
        aria-label={t("youtube.pasteLink")}
        aria-invalid={invalid}
        aria-describedby={invalid ? "youtube-link-error" : undefined}
        className={`min-h-9 w-full rounded-md border border-[var(--color-border-light)] bg-[var(--color-surface)] px-2 text-[13px] text-[var(--color-text)] ${focusRing}`}
      />
      {invalid ? (
        <p
          id="youtube-link-error"
          role="alert"
          className="text-[12px] text-[var(--color-destructive)]"
        >
          {t("youtube.invalidUrl")}
        </p>
      ) : null}
      <div className="flex justify-end gap-2">
        <button
          type="button"
          disabled={busy}
          onClick={() => void submit(true)}
          className={`${buttonBase} border border-[var(--color-border-light)] text-[var(--color-text)]`}
        >
          {t("library.playNow")}
        </button>
        <button
          type="submit"
          disabled={busy}
          className={`${buttonBase} bg-[var(--color-accent)] text-[var(--color-on-accent)]`}
        >
          {t("youtube.addToQueue")}
        </button>
      </div>
    </form>
  );
}
