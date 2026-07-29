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
import { SettingsStemModeSection } from "./SettingsStemModeSection";
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
          "settings.stemMode.label": "Separation Mode",
          "settings.stemMode.description": "Choose how songs are separated.",
          "settings.stemMode.twoStem": "2-stem",
          "settings.stemMode.twoStemDescription": "Vocals and accompaniment.",
          "settings.stemMode.fourStem": "4-stem",
          "settings.stemMode.fourStemDescription":
            "Vocals, drums, bass, and other.",
          "settings.hideUpgradeAll.hide": "Hide Upgrade All to 4-stem",
          "settings.hideUpgradeAll.description":
            "Hide the upgrade button in the sidebar.",
        };
        return map[key] ?? key;
      },
    }),
  };
});

describe("SettingsStemModeSection", () => {
  afterEach(() => {
    cleanup();
  });

  test("renders the stem mode options and hide-upgrade-all checkbox", () => {
    const value = createSettingsOverlayTestContextValue({
      state: {
        stemMode: "two_stem",
        hideUpgradeAll: false,
      },
    });

    const markup = renderToStaticMarkup(
      <SettingsOverlayContext value={value}>
        <SettingsStemModeSection />
      </SettingsOverlayContext>,
    );

    expect(markup).toContain("Separation Mode");
    expect(markup).toContain("2-stem");
    expect(markup).toContain("4-stem");
    expect(markup).toContain("Hide Upgrade All to 4-stem");
  });

  test("checkbox is unchecked when hideUpgradeAll is false", () => {
    const value = createSettingsOverlayTestContextValue({
      state: {
        stemMode: "two_stem",
        hideUpgradeAll: false,
      },
    });

    render(
      <SettingsOverlayContext value={value}>
        <SettingsStemModeSection />
      </SettingsOverlayContext>,
    );

    const checkbox = screen.getByRole("checkbox") as HTMLInputElement;
    expect(checkbox.checked).toBe(false);
  });

  test("checkbox is checked when hideUpgradeAll is true", () => {
    const value = createSettingsOverlayTestContextValue({
      state: {
        stemMode: "four_stem",
        hideUpgradeAll: true,
      },
    });

    render(
      <SettingsOverlayContext value={value}>
        <SettingsStemModeSection />
      </SettingsOverlayContext>,
    );

    const checkbox = screen.getByRole("checkbox") as HTMLInputElement;
    expect(checkbox.checked).toBe(true);
  });

  test("toggling the checkbox calls toggleHideUpgradeAll", () => {
    const toggleHideUpgradeAll = vi.fn().mockResolvedValue(undefined);
    const value = createSettingsOverlayTestContextValue(
      {
        state: {
          stemMode: "two_stem",
          hideUpgradeAll: false,
        },
      },
      { toggleHideUpgradeAll },
    );

    render(
      <SettingsOverlayContext value={value}>
        <SettingsStemModeSection />
      </SettingsOverlayContext>,
    );

    const checkbox = screen.getByRole("checkbox");
    act(() => {
      fireEvent.click(checkbox);
    });

    expect(toggleHideUpgradeAll).toHaveBeenCalledWith(true);
  });
});
