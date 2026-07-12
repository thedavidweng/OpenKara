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
import { SettingsCrossfadeSection } from "./SettingsCrossfadeSection";
import {
  SettingsOverlayContext,
  createSettingsOverlayTestContextValue,
  type SettingsOverlayContextValue,
} from "./SettingsOverlay.context";

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
      <SettingsCrossfadeSection />
    </SettingsOverlayContext>,
  );
}

describe("SettingsCrossfadeSection", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
  });

  test("renders the crossfade section with enable checkbox and duration slider", () => {
    const value = createSettingsOverlayTestContextValue({
      state: {
        crossfadeEnabled: true,
        crossfadeDurationMs: 3000,
      },
      meta: { isInitializing: false },
    });

    renderWithContext(value);

    const enable = screen.getByRole("checkbox");
    expect(enable).toBeChecked();

    const slider = screen.getByRole("slider");
    expect(slider).toBeInTheDocument();
  });

  test("clicking the enable checkbox calls actions.setCrossfadeEnabled", () => {
    const setCrossfadeEnabled = vi.fn();
    const value = createSettingsOverlayTestContextValue(
      {
        state: {
          crossfadeEnabled: false,
          crossfadeDurationMs: 3000,
        },
        meta: { isInitializing: false },
      },
      { setCrossfadeEnabled },
    );

    renderWithContext(value);

    const enable = screen.getByRole("checkbox");
    expect(enable).not.toBeChecked();

    fireEvent.click(enable);
    expect(setCrossfadeEnabled).toHaveBeenCalledWith(true);

    cleanup();
    const valueOn = createSettingsOverlayTestContextValue(
      {
        state: {
          crossfadeEnabled: true,
          crossfadeDurationMs: 3000,
        },
        meta: { isInitializing: false },
      },
      { setCrossfadeEnabled },
    );
    renderWithContext(valueOn);
    fireEvent.click(screen.getByRole("checkbox"));
    expect(setCrossfadeEnabled).toHaveBeenCalledWith(false);
  });

  test("when crossfadeEnabled is false the duration slider is disabled", () => {
    const value = createSettingsOverlayTestContextValue({
      state: {
        crossfadeEnabled: false,
        crossfadeDurationMs: 3000,
      },
      meta: { isInitializing: false },
    });

    renderWithContext(value);

    expect(screen.getByRole("slider")).toBeDisabled();
  });

  test("when crossfadeEnabled is true the duration slider is not disabled", () => {
    const value = createSettingsOverlayTestContextValue({
      state: {
        crossfadeEnabled: true,
        crossfadeDurationMs: 3000,
      },
      meta: { isInitializing: false },
    });

    renderWithContext(value);

    expect(screen.getByRole("slider")).not.toBeDisabled();
  });

  test("changing the slider updates the local draft immediately (visible value changes)", () => {
    const setCrossfadeDurationMs = vi.fn();
    const value = createSettingsOverlayTestContextValue(
      {
        state: {
          crossfadeEnabled: true,
          crossfadeDurationMs: 3000,
        },
        meta: { isInitializing: false },
      },
      { setCrossfadeDurationMs },
    );

    renderWithContext(value);

    const slider = screen.getByRole("slider");

    fireEvent.change(slider, { target: { value: "5000" } });

    // The visible duration readout should reflect 5.0 s before the
    // debounced setCrossfadeDurationMs call fires.
    expect(screen.getByText("5.0 s")).toBeInTheDocument();
    expect(setCrossfadeDurationMs).not.toHaveBeenCalled();

    // Flushing the debounce triggers the batched persistence call.
    act(() => {
      vi.advanceTimersByTime(75);
    });
    expect(setCrossfadeDurationMs).toHaveBeenCalledWith(5000);
  });

  test("uses translation keys settings.crossfade.label, settings.crossfade.enable, settings.crossfade.duration", () => {
    const value = createSettingsOverlayTestContextValue({
      state: {
        crossfadeEnabled: true,
        crossfadeDurationMs: 3000,
      },
      meta: { isInitializing: false },
    });

    renderWithContext(value);

    expect(screen.getByText("settings.crossfade.label")).toBeInTheDocument();
    expect(screen.getByText("settings.crossfade.enable")).toBeInTheDocument();
    expect(screen.getByText("settings.crossfade.duration")).toBeInTheDocument();
  });

  test("when the authoritative crossfadeDurationMs prop changes the local draft re-syncs", () => {
    const setCrossfadeDurationMs = vi.fn();
    const value = createSettingsOverlayTestContextValue(
      {
        state: {
          crossfadeEnabled: true,
          crossfadeDurationMs: 3000,
        },
        meta: { isInitializing: false },
      },
      { setCrossfadeDurationMs },
    );

    const utils = renderWithContext(value);

    // Initially 3.0 s.
    expect(screen.getByText("3.0 s")).toBeInTheDocument();

    // Simulate the authoritative store rolling in a new duration.
    act(() => {
      utils.rerender(
        <SettingsOverlayContext
          value={createSettingsOverlayTestContextValue(
            {
              state: {
                crossfadeEnabled: true,
                crossfadeDurationMs: 7000,
              },
              meta: { isInitializing: false },
            },
            { setCrossfadeDurationMs },
          )}
        >
          <SettingsCrossfadeSection />
        </SettingsOverlayContext>,
      );
    });

    // The draft re-syncs so the visible readout reflects the new duration.
    expect(screen.getByText("7.0 s")).toBeInTheDocument();
  });

  test("onPointerUp on the slider flushes the duration immediately", () => {
    const setCrossfadeDurationMs = vi.fn();
    const value = createSettingsOverlayTestContextValue(
      {
        state: {
          crossfadeEnabled: true,
          crossfadeDurationMs: 3000,
        },
        meta: { isInitializing: false },
      },
      { setCrossfadeDurationMs },
    );

    renderWithContext(value);

    const slider = screen.getByRole("slider");

    fireEvent.change(slider, { target: { value: "5000" } });
    expect(setCrossfadeDurationMs).not.toHaveBeenCalled();

    fireEvent.pointerUp(slider);
    expect(setCrossfadeDurationMs).toHaveBeenCalledWith(5000);

    // The debounce timer should have been cleared — advancing time must
    // not trigger a second call.
    act(() => {
      vi.advanceTimersByTime(75);
    });
    expect(setCrossfadeDurationMs).toHaveBeenCalledTimes(1);
  });

  test("onKeyUp on the slider flushes the duration immediately", () => {
    const setCrossfadeDurationMs = vi.fn();
    const value = createSettingsOverlayTestContextValue(
      {
        state: {
          crossfadeEnabled: true,
          crossfadeDurationMs: 3000,
        },
        meta: { isInitializing: false },
      },
      { setCrossfadeDurationMs },
    );

    renderWithContext(value);

    const slider = screen.getByRole("slider");

    fireEvent.change(slider, { target: { value: "4000" } });
    expect(setCrossfadeDurationMs).not.toHaveBeenCalled();

    fireEvent.keyUp(slider);
    expect(setCrossfadeDurationMs).toHaveBeenCalledWith(4000);

    act(() => {
      vi.advanceTimersByTime(75);
    });
    expect(setCrossfadeDurationMs).toHaveBeenCalledTimes(1);
  });

  test("unmount clears the pending debounce timer so setCrossfadeDurationMs is not called", () => {
    const setCrossfadeDurationMs = vi.fn();
    const value = createSettingsOverlayTestContextValue(
      {
        state: {
          crossfadeEnabled: true,
          crossfadeDurationMs: 3000,
        },
        meta: { isInitializing: false },
      },
      { setCrossfadeDurationMs },
    );

    const utils = renderWithContext(value);

    const slider = screen.getByRole("slider");

    fireEvent.change(slider, { target: { value: "6000" } });
    expect(setCrossfadeDurationMs).not.toHaveBeenCalled();

    utils.unmount();

    act(() => {
      vi.advanceTimersByTime(75);
    });
    expect(setCrossfadeDurationMs).not.toHaveBeenCalled();
  });

  test("changing the slider twice quickly clears the previous debounce timer", () => {
    const setCrossfadeDurationMs = vi.fn();
    const value = createSettingsOverlayTestContextValue(
      {
        state: {
          crossfadeEnabled: true,
          crossfadeDurationMs: 3000,
        },
        meta: { isInitializing: false },
      },
      { setCrossfadeDurationMs },
    );

    renderWithContext(value);

    const slider = screen.getByRole("slider");

    fireEvent.change(slider, { target: { value: "4000" } });
    fireEvent.change(slider, { target: { value: "8000" } });

    act(() => {
      vi.advanceTimersByTime(75);
    });
    expect(setCrossfadeDurationMs).toHaveBeenCalledTimes(1);
    expect(setCrossfadeDurationMs).toHaveBeenCalledWith(8000);
  });

  test("formatDuration shows the correct seconds value", () => {
    const value = createSettingsOverlayTestContextValue({
      state: {
        crossfadeEnabled: true,
        crossfadeDurationMs: 4500,
      },
      meta: { isInitializing: false },
    });

    renderWithContext(value);

    expect(screen.getByText("4.5 s")).toBeInTheDocument();
  });
});
