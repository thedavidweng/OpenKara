// @vitest-environment jsdom

import { afterAll, beforeEach, describe, expect, test, vi } from "vitest";

const isPermissionGranted = vi.fn<() => Promise<boolean>>();
const requestPermission = vi.fn<() => Promise<string>>();
const sendNotification = vi.fn();

vi.mock("@tauri-apps/plugin-notification", () => ({
  isPermissionGranted: () => isPermissionGranted(),
  requestPermission: () => requestPermission(),
  sendNotification: (options: unknown) => sendNotification(options),
}));

const { notifyWhenUnfocused } = await import("./notifications");

const hasFocus = vi.spyOn(document, "hasFocus");
const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
afterAll(() => {
  hasFocus.mockRestore();
  warn.mockRestore();
});

beforeEach(() => {
  vi.clearAllMocks();
  isPermissionGranted.mockResolvedValue(true);
  requestPermission.mockResolvedValue("granted");
});

describe("notifyWhenUnfocused", () => {
  test("stays silent while the window is focused", async () => {
    hasFocus.mockReturnValue(true);

    await notifyWhenUnfocused("Separation complete", "Bohemian Rhapsody");

    expect(sendNotification).not.toHaveBeenCalled();
    // The permission prompt must not be raised just because a run finished
    // while the user was looking at it.
    expect(isPermissionGranted).not.toHaveBeenCalled();
    expect(requestPermission).not.toHaveBeenCalled();
  });

  test("sends when unfocused and permission is already granted", async () => {
    hasFocus.mockReturnValue(false);

    await notifyWhenUnfocused("Separation complete", "Bohemian Rhapsody");

    expect(requestPermission).not.toHaveBeenCalled();
    expect(sendNotification).toHaveBeenCalledWith({
      title: "Separation complete",
      body: "Bohemian Rhapsody",
    });
  });

  test("requests permission on demand and sends once it is granted", async () => {
    hasFocus.mockReturnValue(false);
    isPermissionGranted.mockResolvedValue(false);

    await notifyWhenUnfocused("Separation complete", "Bohemian Rhapsody");

    expect(requestPermission).toHaveBeenCalledTimes(1);
    expect(sendNotification).toHaveBeenCalledTimes(1);
  });

  test("degrades silently when the user denies permission", async () => {
    hasFocus.mockReturnValue(false);
    isPermissionGranted.mockResolvedValue(false);
    requestPermission.mockResolvedValue("denied");

    await notifyWhenUnfocused("Separation complete", "Bohemian Rhapsody");

    expect(sendNotification).not.toHaveBeenCalled();
  });

  test("resolves instead of rejecting when the permission API fails", async () => {
    hasFocus.mockReturnValue(false);
    isPermissionGranted.mockRejectedValue(new Error("no notification daemon"));

    await expect(
      notifyWhenUnfocused("Separation complete", "Bohemian Rhapsody"),
    ).resolves.toBeUndefined();
    expect(sendNotification).not.toHaveBeenCalled();
  });
});
