import { renderToStaticMarkup } from "react-dom/server";
import { beforeEach, describe, expect, test, vi } from "vitest";
import { LyricsFontSizeControl } from "./LyricsFontSizeControl";

const { mockSettingsState } = vi.hoisted(() => ({
  mockSettingsState: {
    lyricsFontStep: 0,
    adjustLyricsFontStep: vi.fn(),
    resetLyricsFontStep: vi.fn(),
  },
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

vi.mock("@/stores/settings-store", () => ({
  useSettingsStore: (selector: (state: typeof mockSettingsState) => unknown) =>
    selector(mockSettingsState),
}));

describe("LyricsFontSizeControl", () => {
  beforeEach(() => {
    mockSettingsState.lyricsFontStep = 0;
  });

  test("displays the default medium label at font step zero", () => {
    const markup = renderToStaticMarkup(<LyricsFontSizeControl />);

    expect(markup).toContain(">M<");
    expect(markup).toContain("A-");
    expect(markup).toContain("A+");
    expect(markup).toContain("lyrics.fontSizeResetShort");
  });

  test("displays the large label at font step 1", () => {
    mockSettingsState.lyricsFontStep = 1;

    const markup = renderToStaticMarkup(<LyricsFontSizeControl />);

    expect(markup).toContain(">L<");
  });

  test("displays the extra-small label at font step -2", () => {
    mockSettingsState.lyricsFontStep = -2;

    const markup = renderToStaticMarkup(<LyricsFontSizeControl />);

    expect(markup).toContain(">XS<");
  });

  test("displays the extra-large label at font step 2", () => {
    mockSettingsState.lyricsFontStep = 2;

    const markup = renderToStaticMarkup(<LyricsFontSizeControl />);

    expect(markup).toContain(">XL<");
  });

  test("highlights the step label in white when non-default", () => {
    mockSettingsState.lyricsFontStep = 1;

    const markup = renderToStaticMarkup(<LyricsFontSizeControl />);

    expect(markup).toContain("text-[var(--color-text)]");
  });

  test("uses dimmer text for the step label at default", () => {
    const markup = renderToStaticMarkup(<LyricsFontSizeControl />);

    expect(markup).toContain("text-[var(--color-text)]");
  });

  test("renders accessible labels for the decrease, increase, and reset buttons", () => {
    const markup = renderToStaticMarkup(<LyricsFontSizeControl />);

    expect(markup).toContain("lyrics.fontSizeDecrease");
    expect(markup).toContain("lyrics.fontSizeIncrease");
    expect(markup).toContain("lyrics.fontSizeReset");
  });
});
