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
import { createInitializedSettingsHarness } from "@/test-utils/settings-controller";
import { SettingsControllerContext } from "./SettingsController.context";
import { SettingsCrossfadeSection } from "./SettingsCrossfadeSection";

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

async function createCrossfadeHarness(
  crossfadeEnabled: boolean,
  crossfadeDurationMs = 3_000,
) {
  const harness = await createInitializedSettingsHarness({
    settings: {
      crossfade_enabled: crossfadeEnabled,
      crossfade_duration_ms: crossfadeDurationMs,
    },
  });
  const rendered = render(
    <SettingsControllerContext value={harness.controller}>
      <SettingsCrossfadeSection />
    </SettingsControllerContext>,
  );

  return {
    harness,
    rendered,
    setCrossfadeEnabled: vi.spyOn(
      harness.backend.settings,
      "setCrossfadeEnabled",
    ),
    setCrossfadeDurationMs: vi.spyOn(
      harness.backend.settings,
      "setCrossfadeDurationMs",
    ),
  };
}

describe("SettingsCrossfadeSection", () => {
  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  test("renders the enable checkbox and duration slider", async () => {
    const harness = await createInitializedSettingsHarness({
      settings: { crossfade_enabled: true, crossfade_duration_ms: 5_000 },
    });

    const markup = renderToStaticMarkup(
      <SettingsControllerContext value={harness.controller}>
        <SettingsCrossfadeSection />
      </SettingsControllerContext>,
    );

    expect(markup).toContain("Crossfade");
    expect(markup).toContain("Enable crossfade");
    expect(markup).toContain("Duration");
    expect(markup).toContain("5.0 s");
  });

  test("the checkbox mirrors whether crossfade is enabled", async () => {
    await createCrossfadeHarness(false);
    expect((screen.getByRole("checkbox") as HTMLInputElement).checked).toBe(
      false,
    );
    cleanup();

    await createCrossfadeHarness(true);
    expect((screen.getByRole("checkbox") as HTMLInputElement).checked).toBe(
      true,
    );
  });

  test("toggling the checkbox enables crossfade", async () => {
    const { setCrossfadeEnabled } = await createCrossfadeHarness(false);

    act(() => {
      fireEvent.click(screen.getByRole("checkbox"));
    });

    expect(setCrossfadeEnabled).toHaveBeenCalledWith(true);
  });

  test("changing the slider commits after the debounce", async () => {
    const { setCrossfadeDurationMs } = await createCrossfadeHarness(true);
    vi.useFakeTimers();

    act(() => {
      fireEvent.change(screen.getByRole("slider"), {
        target: { value: "5000" },
      });
    });
    expect(setCrossfadeDurationMs).not.toHaveBeenCalled();

    act(() => {
      vi.advanceTimersByTime(75);
    });

    expect(setCrossfadeDurationMs).toHaveBeenCalledWith(5_000);
    vi.useRealTimers();
  });

  test("pointer release flushes the debounced commit immediately", async () => {
    const { setCrossfadeDurationMs } = await createCrossfadeHarness(true);
    vi.useFakeTimers();

    const slider = screen.getByRole("slider");
    act(() => {
      fireEvent.change(slider, { target: { value: "7000" } });
    });
    expect(setCrossfadeDurationMs).not.toHaveBeenCalled();

    act(() => {
      fireEvent.pointerUp(slider);
    });

    expect(setCrossfadeDurationMs).toHaveBeenCalledWith(7_000);
    vi.useRealTimers();
  });

  test("key release flushes the debounced commit immediately", async () => {
    const { setCrossfadeDurationMs } = await createCrossfadeHarness(true);
    vi.useFakeTimers();

    const slider = screen.getByRole("slider");
    act(() => {
      fireEvent.change(slider, { target: { value: "8000" } });
    });
    expect(setCrossfadeDurationMs).not.toHaveBeenCalled();

    act(() => {
      fireEvent.keyUp(slider);
    });

    expect(setCrossfadeDurationMs).toHaveBeenCalledWith(8_000);
    vi.useRealTimers();
  });

  test("the slider follows whether crossfade is enabled", async () => {
    await createCrossfadeHarness(false);
    expect((screen.getByRole("slider") as HTMLInputElement).disabled).toBe(
      true,
    );
    cleanup();

    await createCrossfadeHarness(true);
    expect((screen.getByRole("slider") as HTMLInputElement).disabled).toBe(
      false,
    );
  });

  test("displays duration in seconds with one decimal place", async () => {
    await createCrossfadeHarness(true, 7_500);

    expect(screen.getByText("7.5 s")).toBeDefined();
  });

  test("a change back to the current value schedules nothing", async () => {
    const { setCrossfadeDurationMs } = await createCrossfadeHarness(true);
    vi.useFakeTimers();

    act(() => {
      fireEvent.change(screen.getByRole("slider"), {
        target: { value: "3000" },
      });
    });
    act(() => {
      vi.advanceTimersByTime(75);
    });

    expect(setCrossfadeDurationMs).not.toHaveBeenCalled();
    vi.useRealTimers();
  });

  test("a second change replaces the pending debounce timer", async () => {
    const { setCrossfadeDurationMs } = await createCrossfadeHarness(true);
    vi.useFakeTimers();

    const slider = screen.getByRole("slider");
    act(() => {
      fireEvent.change(slider, { target: { value: "4000" } });
    });
    act(() => {
      fireEvent.change(slider, { target: { value: "5000" } });
    });
    act(() => {
      vi.advanceTimersByTime(75);
    });

    expect(setCrossfadeDurationMs).toHaveBeenCalledTimes(1);
    expect(setCrossfadeDurationMs).toHaveBeenCalledWith(5_000);
    vi.useRealTimers();
  });

  test("unmount cancels the pending debounced commit without flushing", async () => {
    const { rendered, setCrossfadeDurationMs } =
      await createCrossfadeHarness(true);
    vi.useFakeTimers();

    act(() => {
      fireEvent.change(screen.getByRole("slider"), {
        target: { value: "6000" },
      });
    });
    rendered.unmount();

    act(() => {
      vi.advanceTimersByTime(75);
    });

    expect(setCrossfadeDurationMs).not.toHaveBeenCalled();
    vi.useRealTimers();
  });
});
