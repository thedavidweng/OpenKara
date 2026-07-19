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

describe("VolumeSliders stem popup portal", () => {
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
    mockPlayerState.snapshot.stem_mode = "four_stem";
  });

  afterEach(() => {
    act(() => root.unmount());
    container.remove();
    // Portal nodes attach to document.body — clear leftovers.
    document
      .querySelectorAll("[data-stem-popup]")
      .forEach((node) => node.remove());
  });

  test("expanded tight mixer portals a popup with playback-bar mute chrome", () => {
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

    const popup = document.querySelector(
      '[data-stem-popup="true"]',
    ) as HTMLElement | null;
    expect(popup).not.toBeNull();
    expect(popup?.getAttribute("data-state")).toBe("open");
    expect(popup?.className).toContain("fixed");
    expect(popup?.className).toContain("z-[70]");

    const muteButtons = Array.from(
      document.querySelectorAll("button[aria-label^='Mute ']"),
    ) as HTMLButtonElement[];
    expect(muteButtons.length).toBeGreaterThanOrEqual(4);
    for (const button of muteButtons) {
      expect(button.className).toContain("playback-bar-action-button");
    }
  });

  test("relaxed expand portals sub-stems with longer master-width rails", () => {
    act(() => {
      root.render(<VolumeSliders density="relaxed" />);
    });

    const trigger = container.querySelector(
      'button[data-playback-action="stem-mixer"]',
    ) as HTMLButtonElement;
    expect(trigger).not.toBeNull();

    act(() => {
      trigger.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });

    const popup = document.querySelector('[data-stem-popup="true"]');
    expect(popup).not.toBeNull();
    expect(popup?.className).toContain("p-4");

    const popupRails = Array.from(
      popup!.querySelectorAll("input.audio-level-slider"),
    ) as HTMLInputElement[];
    // Drums + Bass + Other — longer master rail + trailing optical margin.
    expect(popupRails.length).toBe(3);
    for (const rail of popupRails) {
      expect(rail.className).toContain("w-[104px]");
      expect(rail.className).toContain("mr-[14px]");
    }
  });

  test("popup mute button dims the icon when stem is muted", () => {
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
      document.querySelectorAll("button[aria-label^='Unmute ']"),
    ) as HTMLButtonElement[];
    expect(unmuteButtons.length).toBeGreaterThan(0);
    const muted = unmuteButtons[0];
    expect(muted.getAttribute("aria-pressed")).toBe("true");
    expect(muted.getAttribute("data-active")).toBeNull();
    expect(muted.className).toContain("text-[var(--color-text-dimmer)]");
    expect(muted.className).toContain("playback-bar-action-button");
  });
});
