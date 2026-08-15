// @vitest-environment jsdom

import { describe, expect, it } from "vitest";
import { createLanguageTable } from "./i18n-language";

function setNavigatorLanguage(value: string): void {
  Object.defineProperty(navigator, "language", {
    value,
    configurable: true,
  });
}

describe("createLanguageTable", () => {
  it("keeps only loaded codes and preserves the product order", () => {
    const { SUPPORTED_LANGUAGES } = createLanguageTable([
      "zh-CN",
      "en",
      "xx",
      "ja",
    ]);
    expect(SUPPORTED_LANGUAGES.map((language) => language.code)).toEqual([
      "en",
      "zh-CN",
      "ja",
    ]);
  });

  it("falls back within the loaded landing pair", () => {
    const { detectSystemLanguage } = createLanguageTable(["en", "zh-CN"]);
    setNavigatorLanguage("zh-TW");
    expect(detectSystemLanguage()).toBe("zh-CN");
    setNavigatorLanguage("ja-JP");
    expect(detectSystemLanguage()).toBe("en");
  });

  it("keeps a persisted language only when the table loaded it", () => {
    const { resolveAppLanguage } = createLanguageTable(["en", "zh-CN"]);
    expect(resolveAppLanguage("zh-CN", () => "en")).toBe("zh-CN");
    expect(resolveAppLanguage("ja", () => "en")).toBe("en");
  });
});
