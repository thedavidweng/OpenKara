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
 */
export function applyResolvedTheme(
  theme: ResolvedTheme,
  root: HTMLElement = document.documentElement,
): void {
  root.dataset.theme = theme;
  root.style.colorScheme = theme;
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
 */
export function useThemeRuntime(): {
  resolvedTheme: ResolvedTheme;
  startupThemeReady: boolean;
} {
  const themePreference = useSettingsStore((s) => s.themePreference);
  const hydrated = useSettingsStore((s) => s.hydrated);

  const [systemPrefersDark, setSystemPrefersDark] = useState(true);
  const [startupThemeReady, setStartupThemeReady] = useState(false);

  const resolvedTheme = resolveThemePreference(
    themePreference,
    systemPrefersDark,
  );

  // Track the media value in state only because it is an external changing
  // source; do not duplicate resolved theme in state.
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

  // Apply CSS tokens before the browser paints a hydrated frame.
  useLayoutEffect(() => {
    if (!hydrated) {
      return;
    }
    applyResolvedTheme(resolvedTheme);
  }, [hydrated, resolvedTheme]);

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
      // The local bridge may not settle in some embedded environments; keep
      // the CSS theme and mark startup ready so the window is not stranded.
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
    // themePreference and resolvedTheme are intentionally tracked together so
    // a distinct preference/resolved-theme pair calls setTheme once.
  }, [hydrated, themePreference, resolvedTheme]);

  return { resolvedTheme, startupThemeReady };
}

function sanitizeError(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }
  return String(error);
}
