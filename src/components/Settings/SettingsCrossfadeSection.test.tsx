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
import { SettingsCrossfadeSection } from "./SettingsCrossfadeSection";
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
          "settings.crossfade.label": "Crossfade",
          "settings.crossfade.enable": "Enable crossfade",
          "settings.crossfade.description":
            "Overlap the end of one track with the start of the next.",
          "settings.crossfade.duration": "Duration",
        };
        return map[key] ?? key;
      },
    }),
  };
});

describe("SettingsCrossfadeSection", () => {
  afterEach(() => {
    cleanup();
  });

  test("renders the enable checkbox and duration slider", () => {
    const value = createSettingsOverlayTestContextValue({
      state: {
        crossfadeEnabled: true,
        crossfadeDurationMs: 5_000,
      },
    });

    const markup = renderToStaticMarkup(
      <SettingsOverlayContext value={value}>
        <SettingsCrossfadeSection />
      </SettingsOverlayContext>,
    );

    expect(markup).toContain("Crossfade");
    expect(markup).toContain("Enable crossfade");
    expect(markup).toContain("Duration");
    expect(markup).toContain("5.0 s");
  });

  test("checkbox is unchecked when crossfade is disabled", () => {
    const value = createSettingsOverlayTestContextValue({
      state: {
        crossfadeEnabled: false,
        crossfadeDurationMs: 3_000,
      },
    });

    render(
      <SettingsOverlayContext value={value}>
        <SettingsCrossfadeSection />
      </SettingsOverlayContext>,
    );

    const checkbox = screen.getByRole("checkbox") as HTMLInputElement;
    expect(checkbox.checked).toBe(false);
  });

  test("checkbox is checked when crossfade is enabled", () => {
    const value = createSettingsOverlayTestContextValue({
      state: {
        crossfadeEnabled: true,
        crossfadeDurationMs: 3_000,
      },
    });

    render(
      <SettingsOverlayContext value={value}>
        <SettingsCrossfadeSection />
      </SettingsOverlayContext>,
    );

    const checkbox = screen.getByRole("checkbox") as HTMLInputElement;
    expect(checkbox.checked).toBe(true);
  });

  test("toggling the checkbox calls setCrossfadeEnabled", () => {
    const setCrossfadeEnabled = vi.fn().mockResolvedValue(undefined);
    const value = createSettingsOverlayTestContextValue(
      {
        state: {
          crossfadeEnabled: false,
          crossfadeDurationMs: 3_000,
        },
      },
      { setCrossfadeEnabled },
    );

    render(
      <SettingsOverlayContext value={value}>
        <SettingsCrossfadeSection />
      </SettingsOverlayContext>,
    );

    const checkbox = screen.getByRole("checkbox");
    act(() => {
      fireEvent.click(checkbox);
    });

    expect(setCrossfadeEnabled).toHaveBeenCalledWith(true);
  });

  test("changing the slider calls setCrossfadeDurationMs after debounce", () => {
    vi.useFakeTimers();
    const setCrossfadeDurationMs = vi.fn().mockResolvedValue(undefined);
    const value = createSettingsOverlayTestContextValue(
      {
        state: {
          crossfadeEnabled: true,
          crossfadeDurationMs: 3_000,
        },
      },
      { setCrossfadeDurationMs },
    );

    render(
      <SettingsOverlayContext value={value}>
        <SettingsCrossfadeSection />
      </SettingsOverlayContext>,
    );

    const slider = screen.getByRole("slider");
    act(() => {
      fireEvent.change(slider, { target: { value: "5000" } });
    });

    // The commit is debounced — it should not fire immediately.
    expect(setCrossfadeDurationMs).not.toHaveBeenCalled();

    // After the debounce window, the commit fires.
    act(() => {
      vi.advanceTimersByTime(75);
    });

    expect(setCrossfadeDurationMs).toHaveBeenCalledWith(5_000);
    vi.useRealTimers();
  });

  test("slider is disabled when crossfade is disabled", () => {
    const value = createSettingsOverlayTestContextValue({
      state: {
        crossfadeEnabled: false,
        crossfadeDurationMs: 3_000,
      },
    });

    render(
      <SettingsOverlayContext value={value}>
        <SettingsCrossfadeSection />
      </SettingsOverlayContext>,
    );

    const slider = screen.getByRole("slider") as HTMLInputElement;
    expect(slider.disabled).toBe(true);
  });

  test("slider is enabled when crossfade is enabled", () => {
    const value = createSettingsOverlayTestContextValue({
      state: {
        crossfadeEnabled: true,
        crossfadeDurationMs: 3_000,
      },
      meta: { isInitializing: false },
    });

    render(
      <SettingsOverlayContext value={value}>
        <SettingsCrossfadeSection />
      </SettingsOverlayContext>,
    );

    const slider = screen.getByRole("slider") as HTMLInputElement;
    expect(slider.disabled).toBe(false);
  });

  test("displays duration in seconds with one decimal place", () => {
    const value = createSettingsOverlayTestContextValue({
      state: {
        crossfadeEnabled: true,
        crossfadeDurationMs: 7_500,
      },
    });

    render(
      <SettingsOverlayContext value={value}>
        <SettingsCrossfadeSection />
      </SettingsOverlayContext>,
    );

    expect(screen.getByText("7.5 s")).toBeDefined();
  });
});
