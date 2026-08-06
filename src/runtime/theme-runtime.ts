import { useEffect, useLayoutEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useSettingsStore } from "@/stores/settings-store";
import type { ResolvedTheme, ThemePreference } from "@/types/ipc";

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

// Audience presentation keeps color-scheme dark for native control chrome.
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
