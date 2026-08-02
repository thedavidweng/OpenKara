import { describe, expect, test } from "vitest";
import {
  getRemoteProviderLabel,
  getRemoteProviderBrowserSignInOpenedMessage,
  getRemoteProviderAuthTimeoutMessage,
  getRemoteLibraryConnectedMessage,
} from "./remote-library-copy";
import type { RemoteLibraryProvider } from "@/types/ipc";
import type { TFunction } from "i18next";

const translations: Record<string, string> = {
  "setup.remoteProvider.googleDrive.title": "Google Drive",
  "setup.remoteProvider.dropbox.title": "Dropbox",
  "setup.remoteProvider.webdav.title": "WebDAV",
  "settings.library.googleSignInOpened":
    "Google sign-in opened in your browser.",
  "settings.library.dropboxSignInOpened":
    "Dropbox sign-in opened in your browser.",
  "settings.library.googleSignInTimedOut": "Google sign-in timed out.",
  "settings.library.dropboxSignInTimedOut": "Dropbox sign-in timed out.",
  "settings.library.remoteSignInTimedOut": "Remote sign-in timed out.",
  "settings.library.remoteLibraryConnected":
    "Remote library connected successfully.",
};

const t: TFunction = ((key: string) => translations[key] ?? key) as TFunction;

describe("getRemoteProviderLabel", () => {
  test('returns "Google Drive" for google_drive', () => {
    expect(getRemoteProviderLabel(t, "google_drive")).toBe("Google Drive");
  });

  test('returns "Dropbox" for dropbox', () => {
    expect(getRemoteProviderLabel(t, "dropbox")).toBe("Dropbox");
  });

  test('returns "WebDAV" for webdav', () => {
    expect(getRemoteProviderLabel(t, "webdav")).toBe("WebDAV");
  });
});

describe("getRemoteProviderBrowserSignInOpenedMessage", () => {
  test("returns a message for google_drive", () => {
    const msg = getRemoteProviderBrowserSignInOpenedMessage(t, "google_drive");
    expect(msg).not.toBeNull();
    expect(msg).toContain("Google sign-in opened");
  });

  test("returns a message for dropbox", () => {
    const msg = getRemoteProviderBrowserSignInOpenedMessage(t, "dropbox");
    expect(msg).not.toBeNull();
    expect(msg).toContain("Dropbox sign-in opened");
  });

  test("returns null for webdav", () => {
    expect(getRemoteProviderBrowserSignInOpenedMessage(t, "webdav")).toBeNull();
  });
});

describe("getRemoteProviderAuthTimeoutMessage", () => {
  test("returns Google-specific timeout for google_drive", () => {
    const msg = getRemoteProviderAuthTimeoutMessage(t, "google_drive");
    expect(msg).toContain("Google sign-in timed out");
  });

  test("returns Dropbox-specific timeout for dropbox", () => {
    const msg = getRemoteProviderAuthTimeoutMessage(t, "dropbox");
    expect(msg).toContain("Dropbox sign-in timed out");
  });

  test("returns generic timeout for webdav", () => {
    const msg = getRemoteProviderAuthTimeoutMessage(t, "webdav");
    expect(msg).toBe("Remote sign-in timed out.");
  });
});

describe("getRemoteLibraryConnectedMessage", () => {
  test("returns Google Drive connected message for google_drive", () => {
    const msg = getRemoteLibraryConnectedMessage(t, "google_drive");
    expect(msg).toBe("Remote library connected successfully.");
  });

  test("returns Dropbox connected message for dropbox", () => {
    const msg = getRemoteLibraryConnectedMessage(t, "dropbox");
    expect(msg).toBe("Remote library connected successfully.");
  });

  test("returns WebDAV connected message for webdav", () => {
    const msg = getRemoteLibraryConnectedMessage(t, "webdav");
    expect(msg).toBe("Remote library connected successfully.");
  });

  test("returns generic connected message for unknown provider", () => {
    const msg = getRemoteLibraryConnectedMessage(
      t,
      "unknown" as RemoteLibraryProvider,
    );
    expect(msg).toBe("Remote library connected successfully.");
  });
});
