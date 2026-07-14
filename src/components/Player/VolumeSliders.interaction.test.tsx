// @vitest-environment jsdom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { VolumeSliders } from "./VolumeSliders";

const { mockPlayerState, mockLibraryState } = vi.hoisted(() => ({
  mockPlayerState: {
    snapshot: {
      song_id: "song-1",
      has_stems: true,
      stem_mode: "four_stem",
      stem_volumes: {
        vocals: 0.45,
        drums: 0.8,
        bass: 0.35,
        other: 0.55,
      },
    },
    setStemVolume: vi.fn(),
  },
  mockLibraryState: {
    separationStatuses: {
      "song-1": {
        song_id: "song-1",
        state: "completed",
      },
    },
  },
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: { stem?: string }) =>
      (
        ({
          "stems.vocals": "Vocals",
          "stems.accompaniment": "Accompaniment",
          "stems.drums": "Drums",
          "stems.bass": "Bass",
          "stems.other": "Other",
          "stems.expandStems": "Expand stems",
          "stems.collapseStems": "Collapse stems",
          "stems.mute": `Mute ${options?.stem ?? ""}`.trim(),
          "stems.unmute": `Unmute ${options?.stem ?? ""}`.trim(),
        }) as const
      )[key] ?? key,
  }),
}));

vi.mock("@/stores/player-store", () => ({
  usePlayerStore: (selector: (state: typeof mockPlayerState) => unknown) =>
    selector(mockPlayerState),
}));

vi.mock("@/stores/library-store", () => ({
  useLibraryStore: (selector: (state: typeof mockLibraryState) => unknown) =>
    selector(mockLibraryState),
}));

vi.mock("@/components/Overlay/Tooltip", () => ({
  Tooltip: ({ children }: { children: React.ReactNode; label: string }) => (
    <>{children}</>
  ),
}));

describe("VolumeSliders panel stem controls", () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;

  beforeEach(() => {
    (
      globalThis as typeof globalThis & {
        IS_REACT_ACT_ENVIRONMENT?: boolean;
      }
    ).IS_REACT_ACT_ENVIRONMENT = true;
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
    mockPlayerState.snapshot.stem_volumes = {
      vocals: 0.45,
      drums: 0.8,
      bass: 0.35,
      other: 0.55,
    };
    mockPlayerState.snapshot.has_stems = true;
  });

  afterEach(() => {
    act(() => root.unmount());
    container.remove();
  });

  test("expanded tight mixer renders panel mute chrome with aria-pressed", () => {
    act(() => {
      root.render(<VolumeSliders density="tight" />);
    });

    const trigger = container.querySelector(
      'button[data-playback-action="stem-mixer"]',
    ) as HTMLButtonElement;
    expect(trigger).not.toBeNull();
    expect(trigger.getAttribute("aria-pressed")).toBe("false");

    act(() => {
      trigger.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });

    expect(trigger.getAttribute("aria-pressed")).toBe("true");
    expect(trigger.getAttribute("data-active")).toBe("true");

    // Panel-variant stem mute buttons (no playback-bar class) are rendered.
    const muteButtons = Array.from(
      container.querySelectorAll("button[aria-label^='Mute ']"),
    ) as HTMLButtonElement[];
    expect(muteButtons.length).toBeGreaterThanOrEqual(4);
    const panelMute = muteButtons.find(
      (button) => !button.className.includes("playback-bar-action-button"),
    );
    expect(panelMute).toBeDefined();
    expect(panelMute?.getAttribute("aria-pressed")).toBe("false");
    expect(panelMute?.className).toContain(
      "text-[var(--color-control-primary)]",
    );
  });

  test("panel mute button shows accent active chrome when stem is muted", () => {
    mockPlayerState.snapshot.stem_volumes = {
      vocals: 0,
      drums: 0,
      bass: 0.35,
      other: 0.55,
    };

    act(() => {
      root.render(<VolumeSliders density="tight" />);
    });

    const trigger = container.querySelector(
      'button[data-playback-action="stem-mixer"]',
    ) as HTMLButtonElement;
    act(() => {
      trigger.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });

    const unmuteButtons = Array.from(
      container.querySelectorAll("button[aria-label^='Unmute ']"),
    ) as HTMLButtonElement[];
    expect(unmuteButtons.length).toBeGreaterThan(0);
    const panelUnmute = unmuteButtons.find(
      (button) => !button.className.includes("playback-bar-action-button"),
    );
    expect(panelUnmute).toBeDefined();
    expect(panelUnmute?.getAttribute("aria-pressed")).toBe("true");
    expect(panelUnmute?.getAttribute("data-active")).toBe("true");
  });
});
