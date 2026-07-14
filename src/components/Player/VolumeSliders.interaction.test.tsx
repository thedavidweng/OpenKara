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

describe("VolumeSliders expanded mixer trigger", () => {
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
  });

  afterEach(() => {
    act(() => root.unmount());
    container.remove();
  });

  test("expanded tight-density trigger uses theme text token class", () => {
    act(() => {
      root.render(<VolumeSliders density="tight" />);
    });

    const trigger = container.querySelector(
      'button[aria-label="Expand stems"]',
    ) as HTMLButtonElement;
    expect(trigger).not.toBeNull();
    // Closed + stems-available state uses the theme text hover token.
    expect(trigger.className).toContain("hover:text-[var(--color-text)]");

    act(() => {
      trigger.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });

    const collapse = container.querySelector(
      'button[aria-label="Collapse stems"]',
    ) as HTMLButtonElement;
    expect(collapse).not.toBeNull();
    expect(collapse.className).toContain("text-[var(--color-text)]");
  });

  test("inline mute buttons use theme control-primary + text hover tokens", () => {
    act(() => {
      root.render(<VolumeSliders density="relaxed" />);
    });

    const muteButtons = Array.from(
      container.querySelectorAll("button[aria-label^='Mute ']"),
    ) as HTMLButtonElement[];
    expect(muteButtons.length).toBeGreaterThan(0);
    expect(
      muteButtons.some((button) =>
        button.className.includes("text-[var(--color-control-primary)]"),
      ),
    ).toBe(true);
    expect(
      muteButtons.some((button) =>
        button.className.includes("hover:text-[var(--color-text)]"),
      ),
    ).toBe(true);
  });
});
