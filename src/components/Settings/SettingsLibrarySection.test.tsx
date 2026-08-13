// @vitest-environment jsdom

import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test } from "vitest";
import { createInitializedSettingsHarness } from "@/test-utils/settings-controller";
import type { RegisteredLibrary } from "@/types/ipc";
import { SettingsControllerContext } from "./SettingsController.context";
import { SettingsLibrarySection } from "./SettingsLibrarySection";

const localLibrary: RegisteredLibrary = {
  id: "local:/karaoke",
  kind: "local",
  display_name: "Main Library",
  root_path: "/karaoke",
};

const webdavRepository: RegisteredLibrary = {
  id: "remote:drive",
  kind: "remote",
  display_name: "Drive Library",
  provider: "webdav",
  remote_root_locator: "drive-root",
  remote_path_display: "OpenKara / Team Karaoke",
  account_id: "acct-1",
  connection_config: {
    type: "webdav",
    server_url: "https://dav.example.com/remote.php/dav/files/user/",
  },
  cached_db_path: null,
  remote_revision: null,
};

const dropboxRepository: RegisteredLibrary = {
  id: "remote:drive",
  kind: "remote",
  display_name: "Drive Library",
  provider: "dropbox",
  remote_root_locator: "/OpenKara",
  remote_path_display: "/OpenKara",
  account_id: "acct-1",
  connection_config: null,
  cached_db_path: "/tmp/openkara.db",
  remote_revision: null,
};

async function renderSection(options: {
  libraries: RegisteredLibrary[];
  activeLibraryId: string | null;
  libraryError?: string;
}) {
  const harness = await createInitializedSettingsHarness({
    libraries: options.libraries,
    activeLibraryId: options.activeLibraryId,
  });

  if (options.libraryError) {
    harness.librarySession.failOn(
      "switchLibrary",
      new Error(options.libraryError),
    );
    await harness.controller.library.activate("local:/other");
  }

  return renderToStaticMarkup(
    <SettingsControllerContext value={harness.controller}>
      <SettingsLibrarySection />
    </SettingsControllerContext>,
  );
}

describe("SettingsLibrarySection", () => {
  test("renders provider metadata for remote libraries", async () => {
    const markup = await renderSection({
      libraries: [localLibrary, webdavRepository],
      activeLibraryId: webdavRepository.id,
    });

    expect(markup).toContain("Drive Library");
    expect(markup).toContain("WebDAV");
    expect(markup).toContain("OpenKara / Team Karaoke");
    expect(markup).toContain("Add Remote Repository");
  });

  test("renders library management actions separately from switching", async () => {
    const markup = await renderSection({
      libraries: [localLibrary, dropboxRepository],
      activeLibraryId: localLibrary.id,
    });

    expect(markup).toContain("Rename library");
    expect(markup).toContain("Disconnect repository");
    expect(markup).toContain("Delete repository");
    expect(markup).toContain("Refresh remote repository");
    expect(markup).toContain("Reauthorize remote repository");
    expect(markup).not.toContain("Force resync remote library");
    expect(markup).not.toContain("Reconnect provider");
    expect(markup).not.toContain("Update credentials");
  });

  test("renders a library error with the destructive text token", async () => {
    const markup = await renderSection({
      libraries: [localLibrary],
      activeLibraryId: localLibrary.id,
      libraryError: "Failed to switch library",
    });

    expect(markup).toContain("Failed to switch library");
    expect(markup).toContain("text-[var(--color-destructive)]");
  });

  test("active library uses accent selected chrome like stem/EP chips", async () => {
    const markup = await renderSection({
      libraries: [
        localLibrary,
        {
          id: "local:/other",
          kind: "local",
          display_name: "Other Library",
          root_path: "/other",
        },
      ],
      activeLibraryId: localLibrary.id,
    });

    expect(markup).toContain("border-[var(--color-accent)]");
    expect(markup).toContain("bg-[var(--color-accent)]/15");
    expect(markup).toContain("text-[var(--color-accent)]");
    expect(markup).not.toContain(
      "border-[var(--color-control-selected-border)]",
    );
    expect(markup).not.toContain("bg-[var(--color-control-selected-bg)]");
  });
});
