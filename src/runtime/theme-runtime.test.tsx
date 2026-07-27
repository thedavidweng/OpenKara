// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { ThemePreference } from "@/types/ipc";
import {
  applyResolvedTheme,
  resolveThemePreference,
  useThemeRuntime,
} from "./theme-runtime";

const mockSetTheme = vi.fn<(theme: "light" | "dark" | null) => Promise<void>>();
const mockMatchMedia = vi.fn<(query: string) => MediaQueryList>();
const mockConsoleWarn = vi.fn();

let mockThemePreference: ThemePreference = "dark";
let mockHydrated = false;
// When true, the mocked getCurrentWindow throws to exercise the
// createNativeThemeBridge catch path (no Tauri runtime available).
let mockNativeBridgeThrows = false;

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => {
    if (mockNativeBridgeThrows) {
      throw new Error("no native window");
    }
    return { setTheme: mockSetTheme };
  },
}));

vi.mock("@/stores/settings-store", () => ({
  useSettingsStore: (selector: (s: unknown) => unknown) =>
    selector({
      themePreference: mockThemePreference,
      hydrated: mockHydrated,
    }),
}));

vi.stubGlobal("matchMedia", mockMatchMedia);

function createMockMedia(matches: boolean): MediaQueryList {
  return {
    matches,
    media: "(prefers-color-scheme: dark)",
    onchange: null,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    addListener: vi.fn(),
    removeListener: vi.fn(),
    dispatchEvent: vi.fn(),
  } as unknown as MediaQueryList;
}

function createLegacyMockMedia(matches: boolean): MediaQueryList {
  return {
    matches,
    media: "(prefers-color-scheme: dark)",
    onchange: null,
    addListener: vi.fn(),
    removeListener: vi.fn(),
    dispatchEvent: vi.fn(),
  } as unknown as MediaQueryList;
}

// Media object exposing neither addEventListener nor addListener, exercising
// the no-op unsubscribe fallback in subscribeMediaChange.
function createNoopMockMedia(matches: boolean): MediaQueryList {
  return {
    matches,
    media: "(prefers-color-scheme: dark)",
    onchange: null,
    dispatchEvent: vi.fn(),
  } as unknown as MediaQueryList;
}

function setStoreState(preference: ThemePreference, hydrated: boolean) {
  mockThemePreference = preference;
  mockHydrated = hydrated;
}

function renderHook<T>(hook: () => T): {
  result: { current: T };
  unmount: () => void;
} {
  const result = { current: undefined as unknown as T };
  function TestComponent() {
    result.current = hook();
    return null;
  }
  const container = document.createElement("div");
  const root = createRoot(container);
  act(() => {
    root.render(<TestComponent />);
  });
  return {
    result,
    unmount: () => {
      act(() => root.unmount());
    },
  };
}

describe("resolveThemePreference", () => {
  test("returns the explicit preference when not system", () => {
    expect(resolveThemePreference("light", true)).toBe("light");
    expect(resolveThemePreference("dark", false)).toBe("dark");
  });

  test("resolves system to dark when OS prefers dark", () => {
    expect(resolveThemePreference("system", true)).toBe("dark");
  });

  test("resolves system to light when OS prefers light", () => {
    expect(resolveThemePreference("system", false)).toBe("light");
  });
});

describe("applyResolvedTheme", () => {
  test("sets data-theme and color-scheme on the root element", () => {
    const root = document.createElement("html");
    applyResolvedTheme("light", root);
    expect(root.dataset.theme).toBe("light");
    expect(root.style.colorScheme).toBe("light");
  });

  test("applies dark theme attributes", () => {
    const root = document.createElement("html");
    applyResolvedTheme("dark", root);
    expect(root.dataset.theme).toBe("dark");
    expect(root.style.colorScheme).toBe("dark");
  });

  test("forces dark color-scheme when the audience presentation marker is set", () => {
    const root = document.createElement("html");
    root.dataset.presentationMode = "audience";
    applyResolvedTheme("light", root);
    expect(root.dataset.theme).toBe("light");
    // color-scheme must stay dark so native controls render against the dark
    // audience backdrop, even with a saved light preference.
    expect(root.style.colorScheme).toBe("dark");
  });

  test("uses the resolved theme color-scheme when no audience marker is set", () => {
    const root = document.createElement("html");
    applyResolvedTheme("light", root);
    expect(root.style.colorScheme).toBe("light");
  });
});

describe("useThemeRuntime", () => {
  beforeEach(() => {
    mockSetTheme.mockReset();
    mockConsoleWarn.mockReset();
    mockSetTheme.mockResolvedValue(undefined);
    mockThemePreference = "dark";
    mockHydrated = true;
    mockNativeBridgeThrows = false;
    vi.useFakeTimers();
    vi.spyOn(console, "warn").mockImplementation(mockConsoleWarn);
  });

  afterEach(() => {
    vi.useRealTimers();
    document.documentElement.dataset.theme = "";
    document.documentElement.style.colorScheme = "";
    vi.restoreAllMocks();
  });

  test("applies dark theme to document root when preference is dark", () => {
    mockMatchMedia.mockReturnValue(createMockMedia(true));
    setStoreState("dark", true);

    const { unmount } = renderHook(() => {
      return useThemeRuntime();
    });

    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(document.documentElement.style.colorScheme).toBe("dark");
    unmount();
  });

  test("applies light theme to document root when preference is light", () => {
    mockMatchMedia.mockReturnValue(createMockMedia(false));
    setStoreState("light", true);

    const { unmount } = renderHook(() => {
      return useThemeRuntime();
    });

    expect(document.documentElement.dataset.theme).toBe("light");
    expect(document.documentElement.style.colorScheme).toBe("light");
    unmount();
  });

  test("calls native setTheme with the resolved theme for explicit preferences", () => {
    mockMatchMedia.mockReturnValue(createMockMedia(false));
    setStoreState("light", true);

    const { unmount } = renderHook(() => {
      return useThemeRuntime();
    });

    expect(mockSetTheme).toHaveBeenCalledWith("light");
    unmount();
  });

  test("calls native setTheme with null for system preference", () => {
    mockMatchMedia.mockReturnValue(createMockMedia(true));
    setStoreState("system", true);

    const { unmount } = renderHook(() => {
      return useThemeRuntime();
    });

    expect(mockSetTheme).toHaveBeenCalledWith(null);
    unmount();
  });

  test("marks startup ready after native setTheme resolves", () => {
    mockMatchMedia.mockReturnValue(createMockMedia(true));
    setStoreState("dark", true);

    const { result, unmount } = renderHook(() => {
      return useThemeRuntime();
    });

    act(() => {
      vi.runAllTimers();
    });

    expect(result.current.startupThemeReady).toBe(true);
    unmount();
  });

  test("marks startup ready after timeout when setTheme does not settle", () => {
    mockMatchMedia.mockReturnValue(createMockMedia(true));
    setStoreState("dark", true);

    mockSetTheme.mockReturnValue(new Promise(() => {}));

    const { result, unmount } = renderHook(() => {
      return useThemeRuntime();
    });

    act(() => {
      vi.advanceTimersByTime(800);
    });

    expect(result.current.startupThemeReady).toBe(true);
    expect(mockConsoleWarn).toHaveBeenCalledWith(
      expect.stringContaining("did not settle within timeout"),
    );
    unmount();
  });

  test("marks startup ready even when setTheme rejects", async () => {
    mockMatchMedia.mockReturnValue(createMockMedia(true));
    setStoreState("dark", true);

    mockSetTheme.mockRejectedValue(new Error("native failure"));

    const { result, unmount } = renderHook(() => {
      return useThemeRuntime();
    });

    await act(async () => {
      await vi.runAllTimersAsync();
    });

    expect(result.current.startupThemeReady).toBe(true);
    expect(mockConsoleWarn).toHaveBeenCalledWith(
      "[theme] native setTheme rejected",
      "native failure",
    );
    unmount();
  });

  test("resolves system preference to dark when OS prefers dark", () => {
    mockMatchMedia.mockReturnValue(createMockMedia(true));
    setStoreState("system", true);

    const { result, unmount } = renderHook(() => {
      return useThemeRuntime();
    });

    expect(result.current.resolvedTheme).toBe("dark");
    expect(document.documentElement.dataset.theme).toBe("dark");
    unmount();
  });

  test("resolves system preference to light when OS prefers light", () => {
    mockMatchMedia.mockReturnValue(createMockMedia(false));
    setStoreState("system", true);

    const { result, unmount } = renderHook(() => {
      return useThemeRuntime();
    });

    expect(result.current.resolvedTheme).toBe("light");
    expect(document.documentElement.dataset.theme).toBe("light");
    unmount();
  });

  test("does not apply theme or call setTheme before hydration", () => {
    mockMatchMedia.mockReturnValue(createMockMedia(true));
    setStoreState("dark", false);

    const { unmount } = renderHook(() => {
      return useThemeRuntime();
    });

    expect(mockSetTheme).not.toHaveBeenCalled();
    expect(document.documentElement.dataset.theme).toBe("");
    unmount();
  });

  test("defaults system preference to dark when matchMedia is unavailable", () => {
    vi.stubGlobal("matchMedia", undefined);
    setStoreState("system", true);

    const { result, unmount } = renderHook(() => {
      return useThemeRuntime();
    });

    expect(result.current.resolvedTheme).toBe("dark");
    unmount();
    vi.stubGlobal("matchMedia", mockMatchMedia);
  });

  test("defaults system preference to dark when matchMedia throws", () => {
    mockMatchMedia.mockImplementation(() => {
      throw new Error("matchMedia unavailable");
    });
    setStoreState("system", true);

    const { result, unmount } = renderHook(() => {
      return useThemeRuntime();
    });

    expect(result.current.resolvedTheme).toBe("dark");
    unmount();
  });

  test("subscribes via the legacy addListener API when addEventListener is absent", () => {
    const media = createLegacyMockMedia(false);
    mockMatchMedia.mockReturnValue(media);
    setStoreState("system", true);

    const { unmount } = renderHook(() => {
      return useThemeRuntime();
    });

    expect(media.addListener).toHaveBeenCalledWith(expect.any(Function));
    unmount();
  });

  test("returns a no-op unsubscribe when the media object has no listener API", () => {
    const media = createNoopMockMedia(false);
    mockMatchMedia.mockReturnValue(media);
    setStoreState("system", true);

    const { unmount } = renderHook(() => {
      return useThemeRuntime();
    });

    unmount();
  });

  test("updates resolved theme when the system color-scheme media query changes", () => {
    const media = createMockMedia(false);
    mockMatchMedia.mockReturnValue(media);
    setStoreState("system", true);

    const { result, unmount } = renderHook(() => {
      return useThemeRuntime();
    });

    expect(result.current.resolvedTheme).toBe("light");

    // Simulate the OS switching to dark: invoke the registered change handler.
    const changeHandler = (media.addEventListener as ReturnType<typeof vi.fn>)
      .mock.calls[0][1] as () => void;
    (media as { matches: boolean }).matches = true;
    act(() => {
      changeHandler();
    });

    expect(result.current.resolvedTheme).toBe("dark");
    unmount();
  });

  test("marks startup ready immediately when the native bridge is unavailable", () => {
    mockMatchMedia.mockReturnValue(createMockMedia(true));
    setStoreState("dark", true);
    mockNativeBridgeThrows = true;

    const { result, unmount } = renderHook(() => {
      return useThemeRuntime();
    });

    expect(mockSetTheme).not.toHaveBeenCalled();
    expect(result.current.startupThemeReady).toBe(true);
    unmount();
  });

  test("sanitizes non-Error rejection reasons from native setTheme", async () => {
    mockMatchMedia.mockReturnValue(createMockMedia(true));
    setStoreState("dark", true);

    // Reject with a non-Error value to exercise the String(error) branch of
    // sanitizeError.
    mockSetTheme.mockRejectedValue("string failure");

    const { result, unmount } = renderHook(() => {
      return useThemeRuntime();
    });

    await act(async () => {
      await vi.runAllTimersAsync();
    });

    expect(result.current.startupThemeReady).toBe(true);
    expect(mockConsoleWarn).toHaveBeenCalledWith(
      "[theme] native setTheme rejected",
      "string failure",
    );
    unmount();
  });
});
