import { describe, expect, test } from "vitest";
import { NATIVE_LANGUAGE_NAMES } from "@/lib/i18n";
import {
  analyzeReference,
  compareLocale,
  flattenEntries,
  flattenKeys,
} from "../../scripts/i18n-key-check.mjs";
import en from "./en.json";
import zh from "./zh-CN.json";

const localeModules = import.meta.glob("./*.json", {
  eager: true,
  import: "default",
}) as Record<string, Record<string, unknown>>;

function codeFromPath(path: string): string {
  const file = path.slice(path.lastIndexOf("/") + 1);
  return file.slice(0, -".json".length);
}

const LOCALES: Record<string, Record<string, unknown>> = {};
for (const [path, data] of Object.entries(localeModules)) {
  LOCALES[codeFromPath(path)] = data;
}

const REFERENCE = "en";
const referenceAnalysis = analyzeReference(flattenKeys(LOCALES[REFERENCE]));
const otherCodes = Object.keys(LOCALES).filter((code) => code !== REFERENCE);

// i18next resolves count-aware keys through plural suffixes, so treat a key as
// present when either the literal key or any of its plural variants exists.
const PLURAL_SUFFIXES = ["_zero", "_one", "_two", "_few", "_many", "_other"];

function lookup(locale: unknown, key: string): unknown {
  return key
    .split(".")
    .reduce<unknown>(
      (node, part) =>
        node && typeof node === "object"
          ? (node as Record<string, unknown>)[part]
          : undefined,
      locale,
    );
}

function isPresent(locale: unknown, key: string): boolean {
  if (lookup(locale, key) !== undefined) return true;
  return PLURAL_SUFFIXES.some(
    (suffix) => lookup(locale, `${key}${suffix}`) !== undefined,
  );
}

/** The set of `{{variable}}` names used in a string, normalized (trimmed, and
 *  keyed on the name before any `, format` suffix). */
function placeholders(value: string): Set<string> {
  const names = new Set<string>();
  for (const match of value.matchAll(/\{\{\s*([^}]+?)\s*\}\}/g)) {
    names.add(match[1].split(",")[0].trim());
  }
  return names;
}

describe("locale registry", () => {
  test("ships at least English and Simplified Chinese", () => {
    expect(Object.keys(LOCALES)).toEqual(
      expect.arrayContaining(["en", "zh-CN"]),
    );
  });

  test("every locale file has a native name and vice versa", () => {
    const fileCodes = Object.keys(LOCALES).sort();
    const nameCodes = Object.keys(NATIVE_LANGUAGE_NAMES).sort();

    const filesWithoutName = fileCodes.filter(
      (code) => !(code in NATIVE_LANGUAGE_NAMES),
    );
    const namesWithoutFile = nameCodes.filter((code) => !(code in LOCALES));

    expect(
      filesWithoutName,
      `locale files missing a NATIVE_LANGUAGE_NAMES entry: ${filesWithoutName.join(", ")}`,
    ).toEqual([]);
    expect(
      namesWithoutFile,
      `NATIVE_LANGUAGE_NAMES entries with no locale file: ${namesWithoutFile.join(", ")}`,
    ).toEqual([]);
  });
});

describe("locale key structure", () => {
  test.each(otherCodes)(
    "%s matches en.json (same key-structure check as check-i18n)",
    (code) => {
      const { missing, extra } = compareLocale(
        referenceAnalysis,
        flattenKeys(LOCALES[code]),
        code,
      );
      expect(
        missing,
        `${code}.json is missing keys: ${missing.join(", ")}`,
      ).toEqual([]);
      expect(
        extra,
        `${code}.json has keys absent from en.json: ${extra.join(", ")}`,
      ).toEqual([]);
    },
  );
});

describe("locale values", () => {
  test.each(Object.keys(LOCALES))(
    "%s has only non-empty string leaves",
    (code) => {
      const bad = flattenEntries(LOCALES[code]).filter(
        ([, value]) => typeof value !== "string" || value.trim() === "",
      );
      expect(
        bad.map(([key]) => key),
        `${code}.json has empty or non-string values`,
      ).toEqual([]);
    },
  );

  test.each(otherCodes)(
    "%s preserves every {{placeholder}} set relative to en.json",
    (code) => {
      const locale = LOCALES[code];
      const drift: string[] = [];
      for (const [key, value] of flattenEntries(LOCALES[REFERENCE])) {
        if (typeof value !== "string") continue;
        const target = lookup(locale, key);
        if (typeof target !== "string") continue; // key absence is a structure concern
        const expected = [...placeholders(value)].sort();
        const actual = [...placeholders(target)].sort();
        if (JSON.stringify(expected) !== JSON.stringify(actual)) {
          drift.push(`${key}: expected {${expected}} got {${actual}}`);
        }
      }
      expect(drift, `${code}.json placeholder drift`).toEqual([]);
    },
  );
});

describe("locale copy", () => {
  test("uses the approved hide separate-all copy", () => {
    expect(en.settings.hideBatchSeparate.hide).toBe("Hide “Separate All”");
    expect(en.settings.hideBatchSeparate.description).toBe(
      "Hide the sidebar button that separates all songs.",
    );
    expect(zh.settings.hideBatchSeparate.hide).toBe("隐藏“全部分离”按钮");
    expect(zh.settings.hideBatchSeparate.description).toBe(
      "隐藏侧栏中用于分离全部歌曲的按钮。",
    );
  });

  test("uses the approved hide upgrade-all copy", () => {
    expect(en.settings.hideUpgradeAll.hide).toBe(
      "Hide “Upgrade All to 4-stem”",
    );
    expect(en.settings.hideUpgradeAll.description).toBe(
      "Hide the sidebar button that re-separates all songs into 4-stem mode.",
    );
    expect(zh.settings.hideUpgradeAll.hide).toBe("隐藏“全部升级为4轨”按钮");
    expect(zh.settings.hideUpgradeAll.description).toBe(
      "隐藏侧栏中用于将全部歌曲重新分离为4轨模式的按钮。",
    );
  });

  test("uses an action label for the multi-select instrumental menu item", () => {
    expect(en.library.markInstrumentalSelected).toBe(
      "Mark as Instrumental ({{count}})",
    );
    expect(zh.library.markInstrumentalSelected).toBe("标记为伴奏 ({{count}})");
  });

  test("translates the remote-library setup title and upload progress label", () => {
    expect(en.setup.openRemoteLibrary).toBe("Choose a remote provider");
    expect(zh.setup.openRemoteLibrary).toBe("选择远程服务商");
    expect(en.progress.uploadingToRemote).toBe(
      "Publishing to remote repository: {{title}}",
    );
    expect(zh.progress.uploadingToRemote).toBe(
      "正在发布到远程资料库：{{title}}",
    );
  });

  test("defines every Settings → About key in all locales", () => {
    const aboutKeys = [
      "settings.about.label",
      "settings.about.description",
      "settings.about.version",
      "settings.about.build",
      "settings.about.system",
      "settings.about.catalog",
      "settings.about.model",
      "settings.about.runtime",
      "settings.about.executionProvider",
      "settings.about.logFile",
      "settings.about.reportHint",
      "settings.about.copyDebugInfo",
      "settings.about.copied",
    ];
    const missing: string[] = [];
    for (const [code, locale] of Object.entries(LOCALES)) {
      for (const key of aboutKeys) {
        if (!isPresent(locale, key)) missing.push(`${code}:${key}`);
      }
    }
    expect(missing, `Missing About keys: ${missing.join(", ")}`).toEqual([]);
  });
});

const REMOTE_FLOW_SOURCES = import.meta.glob(
  [
    "../components/Settings/SettingsLibrarySection.tsx",
    "../components/Settings/SettingsRemoteCacheSection.tsx",
    "../components/Settings/SettingsRemoteDiagnosticsSection.tsx",
    "../components/Settings/LibrarySetup.tsx",
    "../components/Settings/RemoteLibraryWizard.tsx",
    "../components/Settings/remote-library-copy.ts",
    "../components/Settings/remote-library-flow.ts",
    "../components/Library/SongListItem.tsx",
    "../components/Layout/GlobalProgressBar.tsx",
    "../components/Player/RemoteReconnectIndicator.tsx",
  ],
  { query: "?raw", import: "default", eager: true },
) as Record<string, string>;

describe("remote-library flow i18n completeness", () => {
  const EXPECTED_FILE_COUNT = 10;

  const T_CALL = /\bt\(\s*["']([^"']+)["']/g;

  function collectKeys(): string[] {
    const keys = new Set<string>();
    for (const source of Object.values(REMOTE_FLOW_SOURCES)) {
      for (const match of source.matchAll(T_CALL)) {
        keys.add(match[1]);
      }
    }
    return [...keys].sort();
  }

  test("resolves every remote-library flow source file", () => {
    // Guards against a moved/renamed file silently shrinking the scan surface.
    expect(Object.keys(REMOTE_FLOW_SOURCES)).toHaveLength(EXPECTED_FILE_COUNT);
  });

  test("every referenced key resolves in every locale", () => {
    const keys = collectKeys();
    expect(keys.length).toBeGreaterThan(0);

    const missing: string[] = [];
    for (const [code, locale] of Object.entries(LOCALES)) {
      for (const key of keys) {
        if (!isPresent(locale, key)) missing.push(`${code}:${key}`);
      }
    }

    expect(
      missing,
      `Missing locale keys referenced in the remote-library flow: ${missing.join(", ")}`,
    ).toEqual([]);
  });
});

const BOOTSTRAP_BANNER_SOURCES = import.meta.glob(
  [
    "../components/Bootstrap/ModelBootstrapBanner.tsx",
    "../components/Bootstrap/RuntimeUpdateBanner.tsx",
  ],
  { query: "?raw", import: "default", eager: true },
) as Record<string, string>;

describe("bootstrap banner i18n completeness", () => {
  const EXPECTED_FILE_COUNT = 2;

  const T_CALL = /\bt\(\s*["']([^"']+)["']/g;

  function collectKeys(): string[] {
    const keys = new Set<string>();
    for (const source of Object.values(BOOTSTRAP_BANNER_SOURCES)) {
      for (const match of source.matchAll(T_CALL)) {
        keys.add(match[1]);
      }
    }
    return [...keys].sort();
  }

  test("resolves every bootstrap banner source file", () => {
    expect(Object.keys(BOOTSTRAP_BANNER_SOURCES)).toHaveLength(
      EXPECTED_FILE_COUNT,
    );
  });

  test("every referenced key resolves in every locale", () => {
    const keys = collectKeys();
    expect(keys.length).toBeGreaterThan(0);

    const missing: string[] = [];
    for (const [code, locale] of Object.entries(LOCALES)) {
      for (const key of keys) {
        if (!isPresent(locale, key)) missing.push(`${code}:${key}`);
      }
    }

    expect(
      missing,
      `Missing locale keys referenced in the bootstrap banners: ${missing.join(", ")}`,
    ).toEqual([]);
  });
});
