// @vitest-environment happy-dom
import "@testing-library/jest-dom/vitest";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { SettingsEqSection } from "./SettingsEqSection";
import {
  SettingsOverlayContext,
  createSettingsOverlayTestContextValue,
  type SettingsOverlayContextValue,
} from "./SettingsOverlay.context";

// Match the i18n mock strategy used by the rest of the Settings overlay
// tests: return the translation key verbatim so assertions can check for
// the key strings directly.
vi.mock("react-i18next", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react-i18next")>();

  return {
    ...actual,
    useTranslation: () => ({
      t: (key: string) => key,
      i18n: { changeLanguage: vi.fn() },
    }),
  };
});

function renderWithContext(value: SettingsOverlayContextValue) {
  return render(
    <SettingsOverlayContext value={value}>
      <SettingsEqSection />
    </SettingsOverlayContext>,
  );
}

describe("SettingsEqSection", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
  });

  test("renders the EQ section with enable checkbox and 5 band sliders", () => {
    const value = createSettingsOverlayTestContextValue({
      state: { eqEnabled: true, eqGainsDb: [0, 0, 0, 0, 0] },
      meta: { isInitializing: false },
    });

    renderWithContext(value);

    // Enable checkbox
    const enable = screen.getByRole("checkbox");
    expect(enable).toBeChecked();

    // Five band sliders
    const sliders = screen.getAllByRole("slider");
    expect(sliders).toHaveLength(5);

    // Band labels rendered
    expect(screen.getByText("60 Hz")).toBeInTheDocument();
    expect(screen.getByText("230 Hz")).toBeInTheDocument();
    expect(screen.getByText("910 Hz")).toBeInTheDocument();
    expect(screen.getByText("3.6 kHz")).toBeInTheDocument();
    expect(screen.getByText("14 kHz")).toBeInTheDocument();
  });

  test("clicking the enable checkbox calls actions.setEqEnabled(true) / setEqEnabled(false)", () => {
    const setEqEnabled = vi.fn();
    const value = createSettingsOverlayTestContextValue(
      {
        state: { eqEnabled: false, eqGainsDb: [0, 0, 0, 0, 0] },
        meta: { isInitializing: false },
      },
      { setEqEnabled },
    );

    renderWithContext(value);

    const enable = screen.getByRole("checkbox");
    expect(enable).not.toBeChecked();

    fireEvent.click(enable);
    expect(setEqEnabled).toHaveBeenCalledWith(true);

    // Re-render with the authoritative state flipped on, then click again
    // to disable.
    cleanup();
    const valueOn = createSettingsOverlayTestContextValue(
      {
        state: { eqEnabled: true, eqGainsDb: [0, 0, 0, 0, 0] },
        meta: { isInitializing: false },
      },
      { setEqEnabled },
    );
    renderWithContext(valueOn);
    fireEvent.click(screen.getByRole("checkbox"));
    expect(setEqEnabled).toHaveBeenCalledWith(false);
  });

  test("when eqEnabled is false the band sliders are disabled", () => {
    const value = createSettingsOverlayTestContextValue({
      state: { eqEnabled: false, eqGainsDb: [0, 0, 0, 0, 0] },
      meta: { isInitializing: false },
    });

    renderWithContext(value);

    for (const slider of screen.getAllByRole("slider")) {
      expect(slider).toBeDisabled();
    }
  });

  test("when eqEnabled is true the band sliders are not disabled", () => {
    const value = createSettingsOverlayTestContextValue({
      state: { eqEnabled: true, eqGainsDb: [0, 0, 0, 0, 0] },
      meta: { isInitializing: false },
    });

    renderWithContext(value);

    for (const slider of screen.getAllByRole("slider")) {
      expect(slider).not.toBeDisabled();
    }
  });

  test("changing a slider updates the local draft immediately (visible value changes)", () => {
    const setEqGains = vi.fn();
    const value = createSettingsOverlayTestContextValue(
      {
        state: { eqEnabled: true, eqGainsDb: [0, 0, 0, 0, 0] },
        meta: { isInitializing: false },
      },
      { setEqGains },
    );

    renderWithContext(value);

    const [firstSlider] = screen.getAllByRole("slider");

    fireEvent.change(firstSlider, { target: { value: "6" } });

    // The visible dB readout for the first band should reflect +6.0 dB
    // before the debounced setEqGains call fires.
    expect(screen.getByText("+6.0 dB")).toBeInTheDocument();
    expect(setEqGains).not.toHaveBeenCalled();

    // Flushing the debounce triggers the batched persistence call.
    act(() => {
      vi.advanceTimersByTime(75);
    });
    expect(setEqGains).toHaveBeenCalledWith([6, 0, 0, 0, 0]);
  });

  test("clicking reset calls actions.setEqGains([0, 0, 0, 0, 0])", () => {
    const setEqGains = vi.fn();
    const value = createSettingsOverlayTestContextValue(
      {
        state: {
          eqEnabled: true,
          eqGainsDb: [3, -1, 0, 2, -5],
        },
        meta: { isInitializing: false },
      },
      { setEqGains },
    );

    renderWithContext(value);

    const resetButton = screen.getByRole("button", {
      name: "settings.eq.reset",
    });
    fireEvent.click(resetButton);

    expect(setEqGains).toHaveBeenCalledWith([0, 0, 0, 0, 0]);
  });

  test("uses translation keys settings.eq.label, settings.eq.enable, settings.eq.reset", () => {
    const value = createSettingsOverlayTestContextValue({
      state: { eqEnabled: true, eqGainsDb: [0, 0, 0, 0, 0] },
      meta: { isInitializing: false },
    });

    renderWithContext(value);

    expect(screen.getByText("settings.eq.label")).toBeInTheDocument();
    expect(screen.getByText("settings.eq.enable")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "settings.eq.reset" }),
    ).toBeInTheDocument();
  });

  test("when the authoritative eqGainsDb prop changes the local draft re-syncs", () => {
    const setEqGains = vi.fn();
    const value = createSettingsOverlayTestContextValue(
      {
        state: { eqEnabled: true, eqGainsDb: [0, 0, 0, 0, 0] },
        meta: { isInitializing: false },
      },
      { setEqGains },
    );

    const utils = renderWithContext(value);

    // Initially flat: every band reads 0.0 dB.
    expect(screen.getAllByText("0.0 dB")).toHaveLength(5);

    // Simulate the authoritative store rolling in new gains (e.g. after
    // hydration or an external reset).
    act(() => {
      utils.rerender(
        <SettingsOverlayContext
          value={createSettingsOverlayTestContextValue(
            {
              state: {
                eqEnabled: true,
                eqGainsDb: [4, -2, 0, 1, -3],
              },
              meta: { isInitializing: false },
            },
            { setEqGains },
          )}
        >
          <SettingsEqSection />
        </SettingsOverlayContext>,
      );
    });

    // The draft re-syncs so the visible readouts reflect the new gains.
    expect(screen.getByText("+4.0 dB")).toBeInTheDocument();
    expect(screen.getByText("-2.0 dB")).toBeInTheDocument();
    expect(screen.getByText("+1.0 dB")).toBeInTheDocument();
    expect(screen.getByText("-3.0 dB")).toBeInTheDocument();
  });
});
