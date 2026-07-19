// @vitest-environment jsdom

import userEvent from "@testing-library/user-event";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { SettingsGeneralSection } from "./SettingsGeneralSection";
import {
  SettingsOverlayContext,
  createSettingsOverlayTestContextValue,
} from "./SettingsOverlay.context";

describe("SettingsGeneralSection", () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    (
      globalThis as typeof globalThis & {
        IS_REACT_ACT_ENVIRONMENT?: boolean;
      }
    ).IS_REACT_ACT_ENVIRONMENT = true;
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
    user = userEvent.setup();
  });

  afterEach(() => {
    act(() => root.unmount());
    container.remove();
  });

  test("renders the appearance radio group with the current theme preference checked", () => {
    const value = createSettingsOverlayTestContextValue({
      state: { themePreference: "system" },
    });

    act(() => {
      root.render(
        <SettingsOverlayContext value={value}>
          <SettingsGeneralSection />
        </SettingsOverlayContext>,
      );
    });

    const radios = container.querySelectorAll<HTMLInputElement>(
      'input[name="theme-preference"]',
    );
    expect(radios).toHaveLength(3);
    const systemRadio = [...radios].find((r) => r.value === "system");
    expect(systemRadio?.checked).toBe(true);
  });

  test("calls setThemePreference when a radio is clicked", async () => {
    const setThemePreference = vi.fn(async () => {});
    const value = createSettingsOverlayTestContextValue(
      { state: { themePreference: "dark" }, meta: { isInitializing: false } },
      { setThemePreference },
    );

    act(() => {
      root.render(
        <SettingsOverlayContext value={value}>
          <SettingsGeneralSection />
        </SettingsOverlayContext>,
      );
    });

    const lightRadio = container.querySelector<HTMLInputElement>(
      'input[name="theme-preference"][value="light"]',
    );
    expect(lightRadio).not.toBeNull();

    await user.click(lightRadio!);

    expect(setThemePreference).toHaveBeenCalledWith("light");
  });
});
