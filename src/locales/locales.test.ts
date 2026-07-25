import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";
import { describe, expect, test } from "vitest";
import en from "./en.json";
import zh from "./zh-CN.json";

const here = dirname(fileURLToPath(import.meta.url));
const srcRoot = resolve(here, "..");

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
});

// The parity test in this suite only compares en↔zh key sets, so a key missing
// from BOTH locales passes silently while an inline `defaultValue` masks the gap
// in dev. This guard scans the remote-library flow for static `t("literal")`
// calls and fails when a referenced key (with or without a `defaultValue`) is
// absent from either locale — the exact footgun that shipped English strings to
// zh-CN users (issue #209).
describe("remote-library flow i18n completeness", () => {
  // Files that make up the remote-repository setup, wizard, progress, and
  // diagnostics flow flagged in issue #209.
  const REMOTE_FLOW_FILES = [
    "components/Settings/SettingsLibrarySection.tsx",
    "components/Settings/SettingsRemoteCacheSection.tsx",
    "components/Settings/SettingsRemoteDiagnosticsSection.tsx",
    "components/Settings/LibrarySetup.tsx",
    "components/Settings/RemoteLibraryWizard.tsx",
    "components/Settings/remote-library-copy.ts",
    "components/Settings/remote-library-flow.ts",
    "components/Library/SongListItem.tsx",
    "components/Layout/GlobalProgressBar.tsx",
    "components/Player/RemoteReconnectIndicator.tsx",
  ];

  // Matches `t("literal"` / `t('literal'` (allowing whitespace/newlines after
  // the paren). Dynamically built keys — `t(variable)` — are intentionally
  // skipped because their key is not statically knowable.
  const T_CALL = /\bt\(\s*["']([^"']+)["']/g;

  function collectKeys(): string[] {
    const keys = new Set<string>();
    for (const relative of REMOTE_FLOW_FILES) {
      const source = readFileSync(resolve(srcRoot, relative), "utf8");
      for (const match of source.matchAll(T_CALL)) {
        keys.add(match[1]);
      }
    }
    return [...keys].sort();
  }

  test("every referenced key resolves in en.json and zh-CN.json", () => {
    const keys = collectKeys();
    // Guard against the regex silently matching nothing (e.g. a moved file).
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
