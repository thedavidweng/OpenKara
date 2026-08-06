import { describe, expect, test, vi } from "vitest";
import { loadStartupSettings } from "./settings-runtime";

describe("loadStartupSettings", () => {
  test("calls getSettings, hydrateAppSettings, and changeLanguage in order", async () => {
    const settings = {
      stem_mode: "four_stem",
      model_variant: "htdemucs_ft",
      language: "ja",
      hide_batch_separate: false,
      cover_art_backdrop: true,
      hide_upgrade_all: false,
      lyrics_font_step: 0,
    };

    const getSettings = vi.fn().mockResolvedValue(settings);
    const hydrateAppSettings = vi.fn();
    const changeLanguage = vi.fn().mockResolvedValue(undefined);
    const detectFallbackLanguage = vi.fn(() => "en");

    await loadStartupSettings({
      getSettings,
      hydrateAppSettings,
      changeLanguage,
      detectFallbackLanguage,
    });

    expect(getSettings).toHaveBeenCalledOnce();
    expect(hydrateAppSettings).toHaveBeenCalledWith(settings);
    expect(changeLanguage).toHaveBeenCalledWith("ja");
    expect(detectFallbackLanguage).not.toHaveBeenCalled();
  });

  test("uses the system recommend only when settings.language is null", async () => {
    const settings = {
      stem_mode: "two_stem",
      model_variant: "htdemucs",
      language: null,
      hide_batch_separate: false,
      cover_art_backdrop: true,
      hide_upgrade_all: false,
      lyrics_font_step: 0,
      execution_provider: "xnnpack",
      available_execution_providers: ["cpu", "xnnpack"],
      compatible_execution_providers: ["cpu", "xnnpack"],
    };

    const getSettings = vi.fn().mockResolvedValue(settings);
    const hydrateAppSettings = vi.fn();
    const changeLanguage = vi.fn().mockResolvedValue(undefined);
    const detectFallbackLanguage = vi.fn(() => "ko");

    await loadStartupSettings({
      getSettings,
      hydrateAppSettings,
      changeLanguage,
      detectFallbackLanguage,
    });

    expect(hydrateAppSettings).toHaveBeenCalledWith(settings);
    expect(detectFallbackLanguage).toHaveBeenCalledOnce();
    expect(changeLanguage).toHaveBeenCalledWith("ko");
  });

  test("keeps a stored app language when the OS recommend differs", async () => {
    const settings = {
      stem_mode: "four_stem",
      model_variant: "htdemucs_ft",
      language: "en",
      hide_batch_separate: true,
      cover_art_backdrop: false,
      hide_upgrade_all: true,
      lyrics_font_step: 2,
      execution_provider: "cpu",
      available_execution_providers: ["cpu"],
      compatible_execution_providers: ["cpu"],
    };

    const getSettings = vi.fn().mockResolvedValue(settings);
    const hydrateAppSettings = vi.fn();
    const changeLanguage = vi.fn().mockResolvedValue(undefined);
    const detectFallbackLanguage = vi.fn(() => "zh-CN");

    await loadStartupSettings({
      getSettings,
      hydrateAppSettings,
      changeLanguage,
      detectFallbackLanguage,
    });

    expect(changeLanguage).toHaveBeenCalledWith("en");
    expect(detectFallbackLanguage).not.toHaveBeenCalled();
  });

  test("uses settings.language when it is a non-empty string", async () => {
    const settings = {
      stem_mode: "four_stem",
      model_variant: "htdemucs_ft",
      language: "zh-CN",
      hide_batch_separate: true,
      cover_art_backdrop: false,
      hide_upgrade_all: true,
      lyrics_font_step: 2,
      execution_provider: "cpu",
      available_execution_providers: ["cpu"],
      compatible_execution_providers: ["cpu"],
    };

    const getSettings = vi.fn().mockResolvedValue(settings);
    const hydrateAppSettings = vi.fn();
    const changeLanguage = vi.fn().mockResolvedValue(undefined);
    const detectFallbackLanguage = vi.fn(() => "en");

    await loadStartupSettings({
      getSettings,
      hydrateAppSettings,
      changeLanguage,
      detectFallbackLanguage,
    });

    expect(changeLanguage).toHaveBeenCalledWith("zh-CN");
    expect(detectFallbackLanguage).not.toHaveBeenCalled();
  });

  test("propagates errors from getSettings", async () => {
    const getSettings = vi.fn().mockRejectedValue(new Error("IPC failed"));
    const hydrateAppSettings = vi.fn();
    const changeLanguage = vi.fn();
    const detectFallbackLanguage = vi.fn(() => "en");

    await expect(
      loadStartupSettings({
        getSettings,
        hydrateAppSettings,
        changeLanguage,
        detectFallbackLanguage,
      }),
    ).rejects.toThrow("IPC failed");

    expect(hydrateAppSettings).not.toHaveBeenCalled();
    expect(changeLanguage).not.toHaveBeenCalled();
  });
});
