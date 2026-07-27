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

  test("fast master drags scale sub-stems from a frozen base so they cannot drift", () => {
    mockPlayerState.setStemVolume.mockClear();

    act(() => {
      root.render(<VolumeSliders density="relaxed" />);
    });

    const accompSlider = container.querySelector(
      'input[aria-label="Accompaniment"]',
    ) as HTMLInputElement;
    expect(accompSlider).not.toBeNull();

    const setSliderValue = (value: string) => {
      const setter = Object.getOwnPropertyDescriptor(
        HTMLInputElement.prototype,
        "value",
      )?.set;
      setter?.call(accompSlider, value);
      accompSlider.dispatchEvent(new Event("input", { bubbles: true }));
    };

    // Gesture start freezes the base mix {drums: 0.8, bass: 0.35, other: 0.55}.
    act(() => {
      accompSlider.dispatchEvent(new Event("pointerdown", { bubbles: true }));
    });

    // First event commits immediately (leading edge of the rate limiter).
    act(() => {
      setSliderValue("100");
    });

    // Simulate the async backend lag that caused the historical drift: only
    // drums has been committed to the store snapshot mid-drag.
    mockPlayerState.snapshot.stem_volumes = {
      vocals: 0.45,
      drums: 1,
      bass: 0.35,
      other: 0.55,
    };
    act(() => {
      root.render(<VolumeSliders density="relaxed" />);
    });

    // Further drag events must keep scaling from the frozen base, not the
    // inconsistent live snapshot.
    act(() => {
      setSliderValue("40");
      setSliderValue("80");
    });

    // Releasing the pointer flushes the pending trailing value atomically.
    act(() => {
      window.dispatchEvent(new Event("pointerup"));
    });

    const calls = mockPlayerState.setStemVolume.mock.calls as Array<
      [string, number]
    >;
    expect(calls.length % 3).toBe(0);
    const triples: Array<Record<string, number>> = [];
    for (let i = 0; i < calls.length; i += 3) {
      triples.push(Object.fromEntries(calls.slice(i, i + 3)));
    }

    // Every commit is a consistent triple scaled by one factor from the base.
    expect(triples[0]).toEqual({
      drums: 1,
      bass: 0.35 * (1 / 0.8),
      other: 0.55 * (1 / 0.8),
    });

    expect(triples[triples.length - 1]).toEqual({
      drums: 0.8,
      bass: 0.35,
      other: 0.55,
    });
  });

  test("releases a drag whose slider disappears before the pointer comes up", () => {
    mockPlayerState.setStemVolume.mockClear();

    act(() => {
      root.render(<VolumeSliders density="relaxed" />);
    });

    const accompSlider = container.querySelector(
      'input[aria-label="Accompaniment"]',
    ) as HTMLInputElement;
    const setSliderValue = (value: string) => {
      const setter = Object.getOwnPropertyDescriptor(
        HTMLInputElement.prototype,
        "value",
      )?.set;
      setter?.call(accompSlider, value);
      accompSlider.dispatchEvent(new Event("input", { bubbles: true }));
    };

    act(() => {
      accompSlider.dispatchEvent(new Event("pointerdown", { bubbles: true }));
    });
    act(() => {
      setSliderValue("30");
    });

    mockPlayerState.snapshot.has_stems = false;
    act(() => {
      root.render(<VolumeSliders density="relaxed" />);
    });

    // A new separated song arrives with an even mix.
    mockPlayerState.snapshot.has_stems = true;
    mockPlayerState.snapshot.stem_volumes = {
      vocals: 1,
      drums: 1,
      bass: 1,
      other: 1,
    };
    act(() => {
      root.render(<VolumeSliders density="relaxed" />);
    });

    const nextSlider = container.querySelector(
      'input[aria-label="Accompaniment"]',
    ) as HTMLInputElement;
    expect(nextSlider.value).toBe("100");

    mockPlayerState.setStemVolume.mockClear();
    act(() => {
      nextSlider.dispatchEvent(new Event("pointerdown", { bubbles: true }));
    });
    const setNextValue = (value: string) => {
      const setter = Object.getOwnPropertyDescriptor(
        HTMLInputElement.prototype,
        "value",
      )?.set;
      setter?.call(nextSlider, value);
      nextSlider.dispatchEvent(new Event("input", { bubbles: true }));
    };
    act(() => {
      setNextValue("50");
    });
    // Release flushes the trailing value (back-to-back gestures in a test run
    // land inside the 20ms throttle window).
    act(() => {
      window.dispatchEvent(new Event("pointerup"));
    });

    // Scaled from the NEW mix (all 1) — not the previous song's frozen base.
    const calls = mockPlayerState.setStemVolume.mock.calls as Array<
      [string, number]
    >;
    expect(Object.fromEntries(calls.slice(0, 3))).toEqual({
      drums: 0.5,
      bass: 0.5,
      other: 0.5,
    });
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
