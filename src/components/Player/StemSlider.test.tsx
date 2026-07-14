import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test, vi } from "vitest";
import { Mic2 } from "lucide-react";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: { stem?: string }) =>
      key === "stems.mute"
        ? `Mute ${options?.stem ?? ""}`.trim()
        : key === "stems.unmute"
          ? `Unmute ${options?.stem ?? ""}`.trim()
          : key,
  }),
}));

vi.mock("@/stores/player-store", () => ({
  usePlayerStore: () => null,
}));

vi.mock("@/stores/library-store", () => ({
  useLibraryStore: () => ({}),
}));

vi.mock("@/components/Overlay/Tooltip", () => ({
  Tooltip: ({ children }: { children: React.ReactNode }) => <>{children}</>,
}));

vi.mock("./AudioLevelSlider", () => ({
  AudioLevelSlider: () => <div data-testid="slider" />,
}));

import { StemSlider } from "./VolumeSliders";

describe("StemSlider panel chrome", () => {
  test("disabled panel mute uses dimmer text token", () => {
    const markup = renderToStaticMarkup(
      <StemSlider
        icon={Mic2}
        label="Vocals"
        value={0.5}
        onChange={() => {}}
        disabled
      />,
    );
    expect(markup).toContain("text-[var(--color-text-dimmer)]");
  });

  test("muted operational panel mute uses accent token", () => {
    const markup = renderToStaticMarkup(
      <StemSlider
        icon={Mic2}
        label="Vocals"
        value={0}
        onChange={() => {}}
        onIconClick={() => {}}
      />,
    );
    expect(markup).toContain("text-[var(--color-accent)]");
    expect(markup).toContain('aria-pressed="true"');
  });
});
