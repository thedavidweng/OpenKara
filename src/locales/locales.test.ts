import { describe, expect, test } from "vitest";
import en from "./en.json";
import zh from "./zh-CN.json";

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

  test("defines every Settings → About key in both locales", () => {
    // The About section is the cross-platform version + debug-export surface;
    // a key missing from either locale would ship English (or a raw key) to
    // the other language's users, exactly the gap the completeness guard
    // below the remote-library flow exists to catch.
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
    const missing = aboutKeys
      .map((key) => ({ key, en: isPresent(en, key), zh: isPresent(zh, key) }))
      .filter((entry) => !entry.en || !entry.zh);
    expect(
      missing,
      `Missing About keys: ${missing
        .map((m) => `${m.key} (en=${m.en}, zh=${m.zh})`)
        .join(", ")}`,
    ).toEqual([]);
  });
});

// Load the remote-library flow source files as raw strings at build time via
// Vite's import.meta.glob. This intentionally avoids Node's fs/path modules,
// which are not part of the app's DOM/bundler tsconfig (`tsconfig.json`) and
// would fail the CI `tsc --noEmit` gate. Each pattern is an exact file so the
// keys of the returned record double as a presence check for the flow.
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

// The parity test above only compares en↔zh key sets, so a key missing from
// BOTH locales passes silently while an inline `defaultValue` masks the gap in
// dev. This guard scans the remote-library flow for static `t("literal")` calls
// and fails when a referenced key (with or without a `defaultValue`) is absent
// from either locale — the exact footgun that shipped English strings to zh-CN
// users (issue #209).
describe("remote-library flow i18n completeness", () => {
  const EXPECTED_FILE_COUNT = 10;

  // Matches `t("literal"` / `t('literal'` (allowing whitespace/newlines after
  // the paren). Dynamically built keys — `t(variable)` — are intentionally
  // skipped because their key is not statically knowable.
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

  test("every referenced key resolves in en.json and zh-CN.json", () => {
    const keys = collectKeys();
    expect(keys.length).toBeGreaterThan(0);

    const missing = keys
      .map((key) => ({
        key,
        en: isPresent(en, key),
        zh: isPresent(zh, key),
      }))
      .filter((entry) => !entry.en || !entry.zh);

    expect(
      missing,
      `Missing locale keys referenced in the remote-library flow: ${missing
        .map((m) => `${m.key} (en=${m.en}, zh=${m.zh})`)
        .join(", ")}`,
    ).toEqual([]);
  });
});

// The bootstrap banners (model + runtime) drive the first-run separation UX,
// so their Missing/Downloading/Failed copy must never fall back to a raw
// English `defaultValue` for zh-CN users. Same guard as above, scoped to the
// banner sources so a newly referenced key without a locale entry fails CI
// (issue #226 added the runtime banner's three-state copy).
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

  test("every referenced key resolves in en.json and zh-CN.json", () => {
    const keys = collectKeys();
    expect(keys.length).toBeGreaterThan(0);

    const missing = keys
      .map((key) => ({
        key,
        en: isPresent(en, key),
        zh: isPresent(zh, key),
      }))
      .filter((entry) => !entry.en || !entry.zh);

    expect(
      missing,
      `Missing locale keys referenced in the bootstrap banners: ${missing
        .map((m) => `${m.key} (en=${m.en}, zh=${m.zh})`)
        .join(", ")}`,
    ).toEqual([]);
  });
});
