import { useEffect, useLayoutEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useSettingsStore } from "@/stores/settings-store";
import type { ResolvedTheme, ThemePreference } from "@/types/ipc";

/**
 * Resolve a persisted theme preference to a concrete light/dark value using
 * the OS `prefers-color-scheme` signal. When the preference is `system` and
 * the OS reports a dark preference (or the media query is unavailable), the
 * resolved theme is `dark` to preserve the default product appearance.
 */
export function resolveThemePreference(
  preference: ThemePreference,
  systemPrefersDark: boolean,
): ResolvedTheme {
  return preference === "system"
    ? systemPrefersDark
      ? "dark"
      : "light"
    : preference;
}

/**
 * Apply the resolved theme to the document root so semantic CSS tokens and
 * `color-scheme` match before the browser paints a hydrated frame.
 *
 * The audience/fullscreen presentation stage stays explicitly dark regardless
 * of the primary theme preference. When the `data-presentation-mode="audience"`
 * marker is present, `color-scheme` is kept dark so native controls (scrollbars,
 * form elements) render correctly against the dark audience backdrop.
 */
export function applyResolvedTheme(
  theme: ResolvedTheme,
  root: HTMLElement = document.documentElement,
): void {
  root.dataset.theme = theme;
  const isAudience = root.dataset.presentationMode === "audience";
  root.style.colorScheme = isAudience ? "dark" : theme;
}

interface MediaQueryLike {
  matches: boolean;
  addEventListener?: (type: "change", listener: () => void) => void;
  removeEventListener?: (type: "change", listener: () => void) => void;
  addListener?: (listener: () => void) => void;
  removeListener?: (listener: () => void) => void;
}

interface WindowWithLegacyMedia {
  matchMedia?: (query: string) => MediaQueryLike;
}

function getSystemPrefersDarkMedia(): MediaQueryLike | null {
  const w = window as unknown as WindowWithLegacyMedia;
  if (typeof w.matchMedia !== "function") {
    return null;
  }
  try {
    return w.matchMedia("(prefers-color-scheme: dark)");
  } catch {
    return null;
  }
}

function subscribeMediaChange(
  media: MediaQueryLike,
  handler: () => void,
): () => void {
  if (typeof media.addEventListener === "function") {
    media.addEventListener("change", handler);
    return () => media.removeEventListener?.("change", handler);
  }
  if (typeof media.addListener === "function") {
    media.addListener(handler);
    return () => media.removeListener?.(handler);
  }
  return () => {};
}

interface NativeThemeBridge {
  setTheme: (theme: "light" | "dark" | null) => Promise<unknown>;
}

function createNativeThemeBridge(): NativeThemeBridge | null {
  try {
    const win = getCurrentWindow();
    return {
      setTheme: (theme) => win.setTheme(theme),
    };
  } catch {
    return null;
  }
}

const STARTUP_TIMEOUT_MS = 750;

/**
 * React hook that resolves the persisted theme preference, applies the
 * matching CSS tokens and native window theme before first paint, and keeps
 * them in sync across preference and system-appearance changes.
 *
 * Returns `startupThemeReady` which becomes true after the first native
 * `setTheme` call settles (success or failure) or after a 750ms injected
 * timeout guard. The ready gate ensures the hidden main window is not shown
 * until the document theme is correct.
 *
 * In preview mode (website embedded preview) the surrounding landing page
 * owns `data-theme` on the document root; this hook skips writing it so the
 * landing toggle stays the single source of truth and the mock's default
 * preference cannot desync the preview from the chrome.
 */
export function useThemeRuntime(previewMode = false): {
  resolvedTheme: ResolvedTheme;
  startupThemeReady: boolean;
} {
  const themePreference = useSettingsStore((s) => s.themePreference);
  const hydrated = useSettingsStore((s) => s.hydrated);

  const [systemPrefersDark, setSystemPrefersDark] = useState(() => {
    const media = getSystemPrefersDarkMedia();
    return media ? media.matches : true;
  });
  const [startupThemeReady, setStartupThemeReady] = useState(false);

  const resolvedTheme = resolveThemePreference(
    themePreference,
    systemPrefersDark,
  );

  useEffect(() => {
    if (themePreference !== "system") {
      return;
    }
    const media = getSystemPrefersDarkMedia();
    if (!media) {
      setSystemPrefersDark(true);
      return;
    }
    setSystemPrefersDark(media.matches);
    const unsubscribe = subscribeMediaChange(media, () => {
      setSystemPrefersDark(media.matches);
    });
    return unsubscribe;
  }, [themePreference]);

  useLayoutEffect(() => {
    if (!hydrated || previewMode) {
      return;
    }
    applyResolvedTheme(resolvedTheme);
  }, [hydrated, previewMode, resolvedTheme]);

  // Call native setTheme once per distinct preference/resolved-theme pair and
  // gate the startup ready signal on the first settlement.
  useEffect(() => {
    if (!hydrated) {
      return;
    }

    let cancelled = false;
    let settled = false;
    const bridge = createNativeThemeBridge();

    const markReady = () => {
      if (cancelled || settled) {
        return;
      }
      settled = true;
      clearTimeout(timeoutId);
      setStartupThemeReady(true);
    };

    const timeoutId = setTimeout(() => {
      console.warn(
        "[theme] native setTheme did not settle within timeout; CSS theme remains applied",
      );
      markReady();
    }, STARTUP_TIMEOUT_MS);

    if (!bridge) {
      markReady();
      return () => {
        cancelled = true;
        clearTimeout(timeoutId);
      };
    }

    const nativeArg = themePreference === "system" ? null : resolvedTheme;
    bridge
      .setTheme(nativeArg)
      .then(markReady)
      .catch((error: unknown) => {
        // A native-theme rejection is non-fatal: CSS remains applied and the
        // settings are retained. Emit one sanitized warning.
        console.warn("[theme] native setTheme rejected", sanitizeError(error));
        markReady();
      });

    return () => {
      cancelled = true;
      clearTimeout(timeoutId);
    };
  }, [hydrated, themePreference, resolvedTheme]);

  return { resolvedTheme, startupThemeReady };
}

function sanitizeError(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }
  return String(error);
}
