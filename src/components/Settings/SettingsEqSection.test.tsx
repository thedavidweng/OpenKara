// @vitest-environment jsdom

import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, test, vi } from "vitest";
import type { SettingsController } from "@/lib/settings-controller";
import { createInitializedSettingsHarness } from "@/test-utils/settings-controller";
import { SettingsControllerContext } from "./SettingsController.context";
import { SettingsEqSection } from "./SettingsEqSection";

vi.mock("react-i18next", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react-i18next")>();
  return {
    ...actual,
    useTranslation: () => ({
      t: (key: string, options?: { value?: string }) => {
        const map: Record<string, string> = {
          "settings.eq.label": "Equalizer",
          "settings.eq.enable": "Enable 5-band EQ",
          "settings.eq.description": "Adjusts five frequency bands.",
          "settings.eq.band60": "60 Hz",
          "settings.eq.band230": "230 Hz",
          "settings.eq.band910": "910 Hz",
          "settings.eq.band3600": "3.6 kHz",
          "settings.eq.band14000": "14 kHz",
          "settings.eq.bandNameBass": "Bass",
          "settings.eq.bandNameLowMid": "Low mid",
          "settings.eq.bandNameMid": "Mid",
          "settings.eq.bandNameHighMid": "High mid",
          "settings.eq.bandNameTreble": "Treble",
          "settings.eq.presetLabel": "Preset",
          "settings.eq.presetCustom": "Custom",
          "settings.eq.presetFlat": "Flat",
          "settings.eq.presetVocalBoost": "Vocal Boost",
          "settings.eq.presetBassBoost": "Bass Boost",
          "settings.eq.presetTrebleBoost": "Treble Boost",
          "settings.eq.presetWarm": "Warm",
          "settings.eq.presetBright": "Bright",
          "settings.eq.presetRock": "Rock",
          "settings.eq.presetPop": "Pop",
          "settings.eq.gainValue": "{{value}} dB",
          "settings.eq.minimumGain": "-12 dB",
          "settings.eq.neutralGain": "0",
          "settings.eq.maximumGain": "+12 dB",
          "settings.eq.reset": "Reset to flat",
        };
        const value = map[key] ?? key;
        return value.replace(/{{value}}/g, options?.value ?? "");
      },
    }),
  };
});

type EqGains = [number, number, number, number, number];

async function createEqHarness(
  eqEnabled: boolean,
  eqGainsDb: EqGains = [0, 0, 0, 0, 0],
) {
  const harness = await createInitializedSettingsHarness({
    settings: { eq_enabled: eqEnabled, eq_gains_db: eqGainsDb },
  });
  return {
    harness,
    setEqGains: vi.spyOn(harness.backend.settings, "setEqGains"),
    setEqEnabled: vi.spyOn(harness.backend.settings, "setEqEnabled"),
  };
}

function renderSection(controller: SettingsController) {
  return render(
    <SettingsControllerContext value={controller}>
      <SettingsEqSection />
    </SettingsControllerContext>,
  );
}

describe("SettingsEqSection", () => {
  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  test("renders the enable checkbox and five band sliders", async () => {
    const { harness } = await createEqHarness(true, [0, 3, -6, 0, 12]);

    const markup = renderToStaticMarkup(
      <SettingsControllerContext value={harness.controller}>
        <SettingsEqSection />
      </SettingsControllerContext>,
    );

    expect(markup).toContain("Equalizer");
    expect(markup).toContain("Enable 5-band EQ");
    expect(markup).toContain("60 Hz");
    expect(markup).toContain("230 Hz");
    expect(markup).toContain("910 Hz");
    expect(markup).toContain("3.6 kHz");
    expect(markup).toContain("14 kHz");
    expect(markup).toContain("Reset to flat");
    expect((markup.match(/type="range"/g) ?? []).length).toBe(5);
  });

  test("renders friendly band names alongside the Hz captions", async () => {
    const { harness } = await createEqHarness(true);

    const markup = renderToStaticMarkup(
      <SettingsControllerContext value={harness.controller}>
        <SettingsEqSection />
      </SettingsControllerContext>,
    );

    expect(markup).toContain("Bass");
    expect(markup).toContain("Low mid");
    expect(markup).toContain("Mid");
    expect(markup).toContain("High mid");
    expect(markup).toContain("Treble");
    expect(markup).toContain("60 Hz");
    expect(markup).toContain("-12 dB");
    expect(markup).toContain("+12 dB");
  });

  test("renders preset chips and highlights the matching preset", async () => {
    const { harness } = await createEqHarness(true, [6, 3, 0, 0, 1]);

    renderSection(harness.controller);

    expect(
      screen
        .getByRole("button", { name: "Bass Boost" })
        .getAttribute("aria-pressed"),
    ).toBe("true");
    expect(
      screen.getByRole("button", { name: "Flat" }).getAttribute("aria-pressed"),
    ).toBe("false");
    expect(screen.queryByText("Custom")).toBeNull();
  });

  test("shows the Custom indicator when gains match no preset", async () => {
    const { harness } = await createEqHarness(true, [1.5, -4, 7, 0, 2]);

    renderSection(harness.controller);

    expect(screen.getByText("Custom")).not.toBeNull();
  });

  test("clicking a preset commits its gains", async () => {
    const { harness, setEqGains } = await createEqHarness(true);

    renderSection(harness.controller);
    fireEvent.click(screen.getByRole("button", { name: "Vocal Boost" }));

    expect(setEqGains).toHaveBeenCalledTimes(1);
    expect(setEqGains).toHaveBeenCalledWith([-1, -1, 2, 4, 1]);
  });

  test("clicking a preset cancels a pending debounced band commit", async () => {
    const { harness, setEqGains } = await createEqHarness(
      true,
      [1, 0, 0, 0, 0],
    );
    vi.useFakeTimers();

    const { container } = renderSection(harness.controller);
    const sliders = container.querySelectorAll('input[type="range"]');
    fireEvent.change(sliders[0], { target: { value: "5" } });
    fireEvent.click(screen.getByRole("button", { name: "Flat" }));

    act(() => {
      vi.advanceTimersByTime(75);
    });

    expect(setEqGains).toHaveBeenCalledTimes(1);
    expect(setEqGains).toHaveBeenCalledWith([0, 0, 0, 0, 0]);
    vi.useRealTimers();
  });

  test("shows gain values in dB with sign", async () => {
    const { harness } = await createEqHarness(true, [3, -6, 0, 12, -12]);

    const markup = renderToStaticMarkup(
      <SettingsControllerContext value={harness.controller}>
        <SettingsEqSection />
      </SettingsControllerContext>,
    );

    expect(markup).toContain("+3.0 dB");
    expect(markup).toContain("-6.0 dB");
    expect(markup).toContain("0.0 dB");
    expect(markup).toContain("+12.0 dB");
    expect(markup).toContain("-12.0 dB");
  });

  test("renders disabled sliders when EQ is disabled", async () => {
    const { harness } = await createEqHarness(false);

    const markup = renderToStaticMarkup(
      <SettingsControllerContext value={harness.controller}>
        <SettingsEqSection />
      </SettingsControllerContext>,
    );

    expect((markup.match(/disabled=""/g) ?? []).length).toBeGreaterThanOrEqual(
      5,
    );
  });

  test("toggling the checkbox enables the equaliser", async () => {
    const { harness, setEqEnabled } = await createEqHarness(false);

    renderSection(harness.controller);
    fireEvent.click(screen.getByRole("checkbox"));

    expect(setEqEnabled).toHaveBeenCalledWith(true);
  });

  test("updates local draft immediately on slider change without IPC call", async () => {
    const { harness, setEqGains } = await createEqHarness(true);

    const { container } = renderSection(harness.controller);
    const sliders = container.querySelectorAll('input[type="range"]');
    fireEvent.change(sliders[2], { target: { value: "6" } });

    expect((sliders[2] as HTMLInputElement).value).toBe("6");
    expect(setEqGains).not.toHaveBeenCalled();
  });

  test("flushes pending debounced commit on pointer release", async () => {
    const { harness, setEqGains } = await createEqHarness(true);
    vi.useFakeTimers();

    const { container } = renderSection(harness.controller);
    const sliders = container.querySelectorAll('input[type="range"]');
    fireEvent.change(sliders[1], { target: { value: "3" } });
    fireEvent.pointerUp(sliders[1]);

    expect(setEqGains).toHaveBeenCalledTimes(1);
    expect(setEqGains).toHaveBeenCalledWith([0, 3, 0, 0, 0]);
    vi.useRealTimers();
  });

  test("fires one IPC call after 75ms debounce quiet period", async () => {
    const { harness, setEqGains } = await createEqHarness(true);
    vi.useFakeTimers();

    const { container } = renderSection(harness.controller);
    const sliders = container.querySelectorAll('input[type="range"]');
    fireEvent.change(sliders[0], { target: { value: "2" } });
    fireEvent.change(sliders[0], { target: { value: "4" } });
    fireEvent.change(sliders[0], { target: { value: "6" } });

    expect(setEqGains).not.toHaveBeenCalled();

    act(() => {
      vi.advanceTimersByTime(75);
    });

    expect(setEqGains).toHaveBeenCalledTimes(1);
    expect(setEqGains).toHaveBeenCalledWith([6, 0, 0, 0, 0]);
    vi.useRealTimers();
  });

  test("cancels pending debounced commit on unmount", async () => {
    const { harness, setEqGains } = await createEqHarness(true);
    vi.useFakeTimers();

    const { container, unmount } = renderSection(harness.controller);
    const sliders = container.querySelectorAll('input[type="range"]');
    fireEvent.change(sliders[0], { target: { value: "5" } });
    unmount();

    act(() => {
      vi.advanceTimersByTime(75);
    });

    expect(setEqGains).not.toHaveBeenCalled();
    vi.useRealTimers();
  });

  test("skips update when slider change clamps to the current value", async () => {
    const { harness, setEqGains } = await createEqHarness(
      true,
      [12, 0, 0, 0, 0],
    );
    vi.useFakeTimers();

    const { container } = renderSection(harness.controller);
    const slider = container.querySelectorAll(
      'input[type="range"]',
    )[0] as HTMLInputElement;
    slider.removeAttribute("max");
    fireEvent.change(slider, { target: { value: "15" } });

    act(() => {
      vi.advanceTimersByTime(75);
    });

    expect(setEqGains).not.toHaveBeenCalled();
    vi.useRealTimers();
  });

  test("flushes pending debounced commit on key release", async () => {
    const { harness, setEqGains } = await createEqHarness(true);
    vi.useFakeTimers();

    const { container } = renderSection(harness.controller);
    const sliders = container.querySelectorAll('input[type="range"]');
    fireEvent.change(sliders[3], { target: { value: "-3" } });
    fireEvent.keyUp(sliders[3]);

    expect(setEqGains).toHaveBeenCalledTimes(1);
    expect(setEqGains).toHaveBeenCalledWith([0, 0, 0, -3, 0]);
    vi.useRealTimers();
  });

  test("the reset button flattens the gains", async () => {
    const { harness, setEqGains } = await createEqHarness(
      true,
      [3, 0, 0, 0, 0],
    );

    renderSection(harness.controller);
    fireEvent.click(screen.getByRole("button", { name: "Reset to flat" }));

    expect(setEqGains).toHaveBeenCalledWith([0, 0, 0, 0, 0]);
  });
});
