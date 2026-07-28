// @vitest-environment jsdom

import { afterEach, describe, expect, it } from "vitest";
import i18next, { SUPPORTED_LANGUAGES, detectSystemLanguage } from "./i18n";

const supported = new Set(SUPPORTED_LANGUAGES.map((l) => l.code));

function setNavigatorLanguage(value: string): void {
  Object.defineProperty(navigator, "language", {
    value,
    configurable: true,
  });
}

describe("detectSystemLanguage", () => {
  afterEach(async () => {
    setNavigatorLanguage("en-US");
    await i18next.changeLanguage("en");
  });

  it("returns an exact BCP-47 tag match when the locale ships", () => {
    setNavigatorLanguage("zh-CN");
    expect(detectSystemLanguage()).toBe("zh-CN");
    setNavigatorLanguage("en");
    expect(detectSystemLanguage()).toBe("en");
  });

  it("matches a bare base tag such as ja", () => {
    setNavigatorLanguage("ja");
    expect(detectSystemLanguage()).toBe(supported.has("ja") ? "ja" : "en");
  });

  it("falls back from a regional variant to its base (de-AT -> de)", () => {
    setNavigatorLanguage("de-AT");
    expect(detectSystemLanguage()).toBe(supported.has("de") ? "de" : "en");
  });

  it("routes simplified / mainland Chinese to zh-CN", () => {
    for (const tag of ["zh", "zh-CN", "zh-SG", "zh-Hans", "zh-Hans-CN"]) {
      setNavigatorLanguage(tag);
      expect(detectSystemLanguage()).toBe("zh-CN");
    }
  });

  it("routes traditional Chinese to zh-TW once it ships, else to zh-CN", () => {
    const expected = supported.has("zh-TW") ? "zh-TW" : "zh-CN";
    for (const tag of ["zh-TW", "zh-HK", "zh-MO", "zh-Hant", "zh-Hant-HK"]) {
      setNavigatorLanguage(tag);
      expect(detectSystemLanguage()).toBe(expected);
    }
  });

  it("routes any Portuguese to pt-BR once it ships, else to en", () => {
    const expected = supported.has("pt-BR") ? "pt-BR" : "en";
    for (const tag of ["pt", "pt-BR", "pt-PT"]) {
      setNavigatorLanguage(tag);
      expect(detectSystemLanguage()).toBe(expected);
    }
  });

  it("falls back to en for an unknown language", () => {
    setNavigatorLanguage("xx-YY");
    expect(detectSystemLanguage()).toBe("en");
  });

  it("uses canonical BCP-47 tags for every shipped locale", () => {
    for (const { code } of SUPPORTED_LANGUAGES) {
      expect(Intl.getCanonicalLocales(code)).toEqual([code]);
    }
  });

  it("updates the document language when the app language changes", async () => {
    await i18next.changeLanguage("zh-CN");
    expect(document.documentElement.lang).toBe("zh-CN");
  });
});
