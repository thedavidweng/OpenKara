import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test, vi } from "vitest";
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
  Tooltip: ({
    children,
    label,
  }: {
    children: React.ReactNode;
    label: string;
  }) => <span data-tooltip-label={label}>{children}</span>,
}));

describe("VolumeSliders", () => {
  test("renders stem sliders with shared tooltip text and without native titles", () => {
    const markup = renderToStaticMarkup(<VolumeSliders />);

    expect(markup).toContain('data-tooltip-label="Vocals 45%"');
    expect(markup).toContain('data-tooltip-label="Accompaniment 80%"');
    expect(markup).toContain("audio-level-slider");
    expect(markup).not.toContain("title=");
  });

  test("collapses inline stem sliders into a mixer trigger in the tight density", () => {
    const markup = renderToStaticMarkup(<VolumeSliders density="tight" />);

    expect(markup).toContain('aria-label="Expand stems"');
    expect(markup).not.toContain('data-stem-popup="true"');
    expect(markup).not.toContain('data-state="open"');
  });

  test("relaxed inline rails use the shared 88px token class", () => {
    const markup = renderToStaticMarkup(<VolumeSliders density="relaxed" />);

    const relaxedRailCount = (
      markup.match(/audio-level-slider shrink-0 w-\[88px\]/g) ?? []
    ).length;
    expect(relaxedRailCount).toBe(2);
  });

  test("compact inline rails use the shared 72px token class", () => {
    const markup = renderToStaticMarkup(<VolumeSliders density="compact" />);

    const compactRailCount = (
      markup.match(/audio-level-slider shrink-0 w-\[72px\]/g) ?? []
    ).length;
    expect(compactRailCount).toBe(2);
  });

  test("tight density renders no inline audio sliders while closed", () => {
    const markup = renderToStaticMarkup(<VolumeSliders density="tight" />);

    expect(markup).toContain('data-playback-action="stem-mixer"');
    expect(markup).not.toContain("audio-level-slider");
    expect(markup).not.toContain("w-16");
  });

  test("inline vocals/accompaniment use the shared 44px variant with 18px icons", () => {
    const markup = renderToStaticMarkup(<VolumeSliders />);

    expect(markup).toContain('data-playback-action="vocals-mute"');
    expect(markup).toContain('data-playback-action="accompaniment-mute"');
    expect(markup).toContain('width="18"');
    expect(markup).toContain('height="18"');
    expect(markup).toContain('aria-pressed="false"');
  });

  test("inline stem mute button dims the icon when the stem is muted", () => {
    mockPlayerState.snapshot.stem_volumes = {
      vocals: 0,
      drums: 0.8,
      bass: 0.35,
      other: 0.55,
    };
    const markup = renderToStaticMarkup(<VolumeSliders />);

    const vocalsBtn = markup.match(
      /<button[^>]*data-playback-action="vocals-mute"[^>]*>/,
    )?.[0];
    expect(vocalsBtn).toBeDefined();
    expect(vocalsBtn).toContain('aria-pressed="true"');
    expect(vocalsBtn).not.toContain('data-active="true"');
    expect(vocalsBtn).toContain("text-[var(--color-text-dimmer)]");
    mockPlayerState.snapshot.stem_volumes = {
      vocals: 0.45,
      drums: 0.8,
      bass: 0.35,
      other: 0.55,
    };
  });

  test("disabled inline stem buttons keep 44px layout and omit pressed semantics", () => {
    mockPlayerState.snapshot.has_stems = false;
    const markup = renderToStaticMarkup(<VolumeSliders />);

    expect(markup).toContain("playback-bar-action-button");
    expect(markup).not.toContain('aria-pressed="true"');
    expect(markup).not.toContain('data-active="true"');
    expect(markup).toContain('disabled=""');
    mockPlayerState.snapshot.has_stems = true;
  });

  test("tight mixer trigger uses the shared class with 18px icon", () => {
    const markup = renderToStaticMarkup(<VolumeSliders density="tight" />);

    expect(markup).toContain("playback-bar-action-button");
    expect(markup).toContain('data-playback-action="stem-mixer"');
    expect(markup).toContain('width="18"');
    expect(markup).toContain('height="18"');
  });

  test("tight mixer trigger exposes aria-pressed and data-active when expanded", () => {
    mockPlayerState.snapshot.has_stems = false;
    const markup = renderToStaticMarkup(<VolumeSliders density="tight" />);

    expect(markup).not.toContain('aria-pressed="true"');
    expect(markup).not.toContain('data-active="true"');
    mockPlayerState.snapshot.has_stems = true;
  });

  test("disclosure chevron stays compact beside accompaniment", () => {
    mockPlayerState.snapshot.stem_mode = "four_stem";
    const markup = renderToStaticMarkup(<VolumeSliders />);

    expect(markup).toContain("h-6 w-6");
    expect(markup).toContain("rounded-full");
    expect(markup).toContain('data-playback-action="stem-mixer"');
  });
});
