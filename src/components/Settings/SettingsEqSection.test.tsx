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

  test("updates local draft immediately on slider change without IPC call", () => {
    const setEqGains = vi.fn().mockResolvedValue(undefined);
    const value = createSettingsOverlayTestContextValue(
      {
        state: { eqEnabled: true, eqGainsDb: [0, 0, 0, 0, 0] },
        meta: { isInitializing: false },
      },
      { setEqGains },
    );

    const { container } = render(
      <SettingsOverlayContext value={value}>
        <SettingsEqSection />
      </SettingsOverlayContext>,
    );

    const sliders = container.querySelectorAll('input[type="range"]');
    fireEvent.change(sliders[2], { target: { value: "6" } });

    // Local draft updates immediately — the slider's value attribute reflects 6.
    expect((sliders[2] as HTMLInputElement).value).toBe("6");
    // IPC call is debounced — not fired synchronously.
    expect(setEqGains).not.toHaveBeenCalled();
  });

  test("flushes pending debounced commit on pointer release", async () => {
    vi.useFakeTimers();
    const setEqGains = vi.fn().mockResolvedValue(undefined);
    const value = createSettingsOverlayTestContextValue(
      {
        state: { eqEnabled: true, eqGainsDb: [0, 0, 0, 0, 0] },
        meta: { isInitializing: false },
      },
      { setEqGains },
    );

    const { container } = render(
      <SettingsOverlayContext value={value}>
        <SettingsEqSection />
      </SettingsOverlayContext>,
    );

    const sliders = container.querySelectorAll('input[type="range"]');
    fireEvent.change(sliders[1], { target: { value: "3" } });
    // Before the debounce window elapses, flush via pointer release.
    fireEvent.pointerUp(sliders[1]);

    expect(setEqGains).toHaveBeenCalledTimes(1);
    expect(setEqGains).toHaveBeenCalledWith([0, 3, 0, 0, 0]);
    vi.useRealTimers();
  });

  test("fires one IPC call after 75ms debounce quiet period", async () => {
    vi.useFakeTimers();
    const setEqGains = vi.fn().mockResolvedValue(undefined);
    const value = createSettingsOverlayTestContextValue(
      {
        state: { eqEnabled: true, eqGainsDb: [0, 0, 0, 0, 0] },
        meta: { isInitializing: false },
      },
      { setEqGains },
    );

    const { container } = render(
      <SettingsOverlayContext value={value}>
        <SettingsEqSection />
      </SettingsOverlayContext>,
    );

    const sliders = container.querySelectorAll('input[type="range"]');
    // Simulate dragging through several values rapidly.
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

  test("cancels pending debounced commit on unmount", () => {
    vi.useFakeTimers();
    const setEqGains = vi.fn().mockResolvedValue(undefined);
    const value = createSettingsOverlayTestContextValue(
      {
        state: { eqEnabled: true, eqGainsDb: [0, 0, 0, 0, 0] },
        meta: { isInitializing: false },
      },
      { setEqGains },
    );

    const { container, unmount } = render(
      <SettingsOverlayContext value={value}>
        <SettingsEqSection />
      </SettingsOverlayContext>,
    );

    const sliders = container.querySelectorAll('input[type="range"]');
    fireEvent.change(sliders[0], { target: { value: "5" } });

    // Unmount before the debounce window elapses.
    unmount();

    act(() => {
      vi.advanceTimersByTime(75);
    });

    // The pending commit should have been cancelled, not flushed.
    expect(setEqGains).not.toHaveBeenCalled();
    vi.useRealTimers();
  });

  test("skips update when slider change clamps to the current value", () => {
    vi.useFakeTimers();
    const setEqGains = vi.fn().mockResolvedValue(undefined);
    const value = createSettingsOverlayTestContextValue(
      {
        // Band 0 already at the +12 ceiling.
        state: { eqEnabled: true, eqGainsDb: [12, 0, 0, 0, 0] },
        meta: { isInitializing: false },
      },
      { setEqGains },
    );

    const { container } = render(
      <SettingsOverlayContext value={value}>
        <SettingsEqSection />
      </SettingsOverlayContext>,
    );

    const sliders = container.querySelectorAll('input[type="range"]');
    // jsdom clamps range input values to [min, max], so temporarily remove
    // the max attribute to push a value above the ceiling. handleBandChange
    // clamps it back to 12, matching the current draft — the early-return
    // branch should bail out without scheduling a commit.
    const slider = sliders[0] as HTMLInputElement;
    slider.removeAttribute("max");
    fireEvent.change(slider, { target: { value: "15" } });

    act(() => {
      vi.advanceTimersByTime(75);
    });

    expect(setEqGains).not.toHaveBeenCalled();
    vi.useRealTimers();
  });

  test("flushes pending debounced commit on key release", async () => {
    vi.useFakeTimers();
    const setEqGains = vi.fn().mockResolvedValue(undefined);
    const value = createSettingsOverlayTestContextValue(
      {
        state: { eqEnabled: true, eqGainsDb: [0, 0, 0, 0, 0] },
        meta: { isInitializing: false },
      },
      { setEqGains },
    );

    const { container } = render(
      <SettingsOverlayContext value={value}>
        <SettingsEqSection />
      </SettingsOverlayContext>,
    );

    const sliders = container.querySelectorAll('input[type="range"]');
    fireEvent.change(sliders[3], { target: { value: "-3" } });
    // Before the debounce window elapses, flush via key release.
    fireEvent.keyUp(sliders[3]);

    expect(setEqGains).toHaveBeenCalledTimes(1);
    expect(setEqGains).toHaveBeenCalledWith([0, 0, 0, -3, 0]);
    vi.useRealTimers();
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
