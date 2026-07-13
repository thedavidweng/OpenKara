// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, test, vi } from "vitest";
import { SettingsEqSection } from "./SettingsEqSection";
import {
  SettingsOverlayContext,
  createSettingsOverlayTestContextValue,
} from "./SettingsOverlay.context";

vi.mock("react-i18next", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react-i18next")>();
  return {
    ...actual,
    useTranslation: () => ({
      t: (key: string) => {
        const map: Record<string, string> = {
          "settings.eq.label": "Equalizer",
          "settings.eq.enable": "Enable 5-band EQ",
          "settings.eq.description": "Adjusts five frequency bands.",
          "settings.eq.band60": "60 Hz",
          "settings.eq.band230": "230 Hz",
          "settings.eq.band910": "910 Hz",
          "settings.eq.band3600": "3.6 kHz",
          "settings.eq.band14000": "14 kHz",
          "settings.eq.reset": "Reset to flat",
        };
        return map[key] ?? key;
      },
    }),
  };
});

describe("SettingsEqSection", () => {
  afterEach(() => {
    cleanup();
  });

  test("renders the enable checkbox and five band sliders", () => {
    const value = createSettingsOverlayTestContextValue({
      state: {
        eqEnabled: true,
        eqGainsDb: [0, 3, -6, 0, 12],
      },
    });

    const markup = renderToStaticMarkup(
      <SettingsOverlayContext value={value}>
        <SettingsEqSection />
      </SettingsOverlayContext>,
    );

    expect(markup).toContain("Equalizer");
    expect(markup).toContain("Enable 5-band EQ");
    expect(markup).toContain("60 Hz");
    expect(markup).toContain("230 Hz");
    expect(markup).toContain("910 Hz");
    expect(markup).toContain("3.6 kHz");
    expect(markup).toContain("14 kHz");
    expect(markup).toContain("Reset to flat");
    // Five range inputs
    const sliderCount = (markup.match(/type="range"/g) ?? []).length;
    expect(sliderCount).toBe(5);
  });

  test("shows gain values in dB with sign", () => {
    const value = createSettingsOverlayTestContextValue({
      state: {
        eqEnabled: true,
        eqGainsDb: [3, -6, 0, 12, -12],
      },
    });

    const markup = renderToStaticMarkup(
      <SettingsOverlayContext value={value}>
        <SettingsEqSection />
      </SettingsOverlayContext>,
    );

    expect(markup).toContain("+3.0 dB");
    expect(markup).toContain("-6.0 dB");
    expect(markup).toContain("0.0 dB");
    expect(markup).toContain("+12.0 dB");
    expect(markup).toContain("-12.0 dB");
  });

  test("renders disabled sliders when EQ is disabled", () => {
    const value = createSettingsOverlayTestContextValue({
      state: {
        eqEnabled: false,
        eqGainsDb: [0, 0, 0, 0, 0],
      },
    });

    const markup = renderToStaticMarkup(
      <SettingsOverlayContext value={value}>
        <SettingsEqSection />
      </SettingsOverlayContext>,
    );

    // Sliders should be disabled when EQ is off
    const disabledCount = (markup.match(/disabled=""/g) ?? []).length;
    expect(disabledCount).toBeGreaterThanOrEqual(5);
  });

  test("calls setEqEnabled when checkbox is toggled", () => {
    const setEqEnabled = vi.fn().mockResolvedValue(undefined);
    const value = createSettingsOverlayTestContextValue(
      {
        state: { eqEnabled: false, eqGainsDb: [0, 0, 0, 0, 0] },
        meta: { isInitializing: false },
      },
      { setEqEnabled },
    );

    render(
      <SettingsOverlayContext value={value}>
        <SettingsEqSection />
      </SettingsOverlayContext>,
    );

    const checkbox = screen.getByRole("checkbox");
    fireEvent.click(checkbox);

    expect(setEqEnabled).toHaveBeenCalledWith(true);
  });

  test("calls setEqBandGain when a slider is changed", () => {
    const setEqBandGain = vi.fn().mockResolvedValue(undefined);
    const value = createSettingsOverlayTestContextValue(
      {
        state: { eqEnabled: true, eqGainsDb: [0, 0, 0, 0, 0] },
        meta: { isInitializing: false },
      },
      { setEqBandGain },
    );

    const { container } = render(
      <SettingsOverlayContext value={value}>
        <SettingsEqSection />
      </SettingsOverlayContext>,
    );

    const sliders = container.querySelectorAll('input[type="range"]');
    fireEvent.change(sliders[2], { target: { value: "6" } });

    expect(setEqBandGain).toHaveBeenCalledWith(2, 6);
  });

  test("calls resetEqGains when reset button is clicked", () => {
    const resetEqGains = vi.fn().mockResolvedValue(undefined);
    const value = createSettingsOverlayTestContextValue(
      {
        state: { eqEnabled: true, eqGainsDb: [3, 0, 0, 0, 0] },
        meta: { isInitializing: false },
      },
      { resetEqGains },
    );

    render(
      <SettingsOverlayContext value={value}>
        <SettingsEqSection />
      </SettingsOverlayContext>,
    );

    const resetButton = screen.getByRole("button");
    fireEvent.click(resetButton);

    expect(resetEqGains).toHaveBeenCalled();
  });
});
