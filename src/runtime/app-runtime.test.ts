import { describe, expect, test, vi } from "vitest";
import { loadStartupSettings } from "./settings-runtime";

describe("app runtime settings hydration", () => {
  test("hydrates settings and applies the persisted language", async () => {
    const getSettings = vi.fn().mockResolvedValue({
      stem_mode: "four_stem",
      model_variant: "htdemucs_ft",
      language: "zh-CN",
      hide_batch_separate: true,
      cover_art_backdrop: true,
      lyrics_blur_inactive: false,
      hide_upgrade_all: true,
      lyrics_font_step: 1,
      theme_preference: "dark",
    });
    const hydrateAppSettings = vi.fn();
    const changeLanguage = vi.fn().mockResolvedValue(undefined);
    const detectFallbackLanguage = vi.fn(() => "en");

    await loadStartupSettings({
      getSettings,
      hydrateAppSettings,
      changeLanguage,
      detectFallbackLanguage,
    });

    expect(hydrateAppSettings).toHaveBeenCalledWith({
      stem_mode: "four_stem",
      model_variant: "htdemucs_ft",
      language: "zh-CN",
      hide_batch_separate: true,
      cover_art_backdrop: true,
      lyrics_blur_inactive: false,
      hide_upgrade_all: true,
      lyrics_font_step: 1,
      theme_preference: "dark",
    });
    expect(changeLanguage).toHaveBeenCalledWith("zh-CN");
    expect(detectFallbackLanguage).not.toHaveBeenCalled();
  });

  test("falls back to the detected system language when none is saved", async () => {
    const getSettings = vi.fn().mockResolvedValue({
      stem_mode: "two_stem",
      model_variant: "htdemucs",
      language: null,
      hide_batch_separate: false,
      cover_art_backdrop: true,
      lyrics_blur_inactive: false,
      hide_upgrade_all: false,
      lyrics_font_step: 0,
      execution_provider: "xnnpack",
      available_execution_providers: ["cpu", "xnnpack"],
      compatible_execution_providers: ["cpu", "xnnpack"],
      theme_preference: "dark",
    });
    const hydrateAppSettings = vi.fn();
    const changeLanguage = vi.fn().mockResolvedValue(undefined);
    const detectFallbackLanguage = vi.fn(() => "ja");

    await loadStartupSettings({
      getSettings,
      hydrateAppSettings,
      changeLanguage,
      detectFallbackLanguage,
    });

    expect(hydrateAppSettings).toHaveBeenCalledOnce();
    expect(detectFallbackLanguage).toHaveBeenCalledOnce();
    expect(changeLanguage).toHaveBeenCalledWith("ja");
  });
});
