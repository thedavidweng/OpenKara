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
import { SettingsStemModeSection } from "./SettingsStemModeSection";

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
    vi.restoreAllMocks();
  });

  test("renders the stem mode options and hide-upgrade-all checkbox", async () => {
    const harness = await createInitializedSettingsHarness({
      settings: { stem_mode: "two_stem", hide_upgrade_all: false },
    });

    const markup = renderToStaticMarkup(
      <SettingsControllerContext value={harness.controller}>
        <SettingsStemModeSection />
      </SettingsControllerContext>,
    );

    expect(markup).toContain("Separation Mode");
    expect(markup).toContain("2-stem");
    expect(markup).toContain("4-stem");
    expect(markup).toContain("Hide Upgrade All to 4-stem");
  });

  test("the checkbox mirrors the stored hide-upgrade-all preference", async () => {
    const off = await createInitializedSettingsHarness({
      settings: { hide_upgrade_all: false },
    });
    render(
      <SettingsControllerContext value={off.controller}>
        <SettingsStemModeSection />
      </SettingsControllerContext>,
    );
    expect((screen.getByRole("checkbox") as HTMLInputElement).checked).toBe(
      false,
    );
    cleanup();

    const on = await createInitializedSettingsHarness({
      settings: { hide_upgrade_all: true },
    });
    render(
      <SettingsControllerContext value={on.controller}>
        <SettingsStemModeSection />
      </SettingsControllerContext>,
    );
    expect((screen.getByRole("checkbox") as HTMLInputElement).checked).toBe(
      true,
    );
  });

  test("toggling the checkbox writes the preference", async () => {
    const harness = await createInitializedSettingsHarness({
      settings: { hide_upgrade_all: false },
    });
    const setHideUpgradeAll = vi.spyOn(
      harness.backend.settings,
      "setHideUpgradeAll",
    );

    render(
      <SettingsControllerContext value={harness.controller}>
        <SettingsStemModeSection />
      </SettingsControllerContext>,
    );
    act(() => {
      fireEvent.click(screen.getByRole("checkbox"));
    });

    expect(setHideUpgradeAll).toHaveBeenCalledWith(true);
  });
});
