import { beforeEach, describe, expect, test, vi } from "vitest";

const { mockAddNotification, mockT } = vi.hoisted(() => ({
  mockAddNotification: vi.fn(),
  mockT: vi.fn((key: string) => key),
}));

vi.mock("@/stores/notification-store", () => ({
  useNotificationStore: {
    getState: () => ({
      addNotification: mockAddNotification,
    }),
  },
}));

vi.mock("@/lib/i18n", () => ({
  default: { t: mockT },
}));

import { getErrorMessage, notifyError, notifySuccess } from "./errors";
import type { CommandError } from "@/types/ipc";

function commandError(overrides: Partial<CommandError> = {}): CommandError {
  return {
    code: "internal",
    message: "Something broke",
    retryable: false,
    fallback: "stay_in_original_mode",
    ...overrides,
  };
}

describe("getErrorMessage", () => {
  test("returns message from CommandError", () => {
    const err = commandError({ message: "db is down" });
    expect(getErrorMessage(err)).toBe("db is down");
  });

  test("localizes the incompatible provider recovery message", () => {
    const err = commandError({
      code: "execution_provider_unavailable",
      message: "technical provider detail",
    });

    expect(getErrorMessage(err)).toBe(
      "errors.executionProviderUnavailableMessage",
    );
    expect(mockT).toHaveBeenCalledWith(
      "errors.executionProviderUnavailableMessage",
    );
  });

  test("localizes runtime/model bootstrap errors instead of exposing backend language", () => {
    const err = commandError({
      code: "model_unavailable",
      message: "ONNX Runtime is still downloading to C:\\OpenKara",
    });

    expect(getErrorMessage(err)).toBe("errors.modelUnavailableMessage");
    expect(mockT).toHaveBeenCalledWith("errors.modelUnavailableMessage");
  });

  test("returns message from Error instance", () => {
    expect(getErrorMessage(new Error("boom"))).toBe("boom");
  });

  test("returns message from plain object with message property", () => {
    expect(getErrorMessage({ message: "plain obj" })).toBe("plain obj");
  });

  test("returns String() for a string primitive", () => {
    expect(getErrorMessage("raw string")).toBe("raw string");
  });

  test("returns String() for null", () => {
    expect(getErrorMessage(null)).toBe("null");
  });

  test("returns String() for undefined", () => {
    expect(getErrorMessage(undefined)).toBe("undefined");
  });

  test("returns String() for a number", () => {
    expect(getErrorMessage(42)).toBe("42");
  });
});

describe("notifyError", () => {
  beforeEach(() => {
    mockAddNotification.mockClear();
    mockT.mockClear();
    mockT.mockImplementation((key: string) => key);
  });

  test("CommandError with retryable=true includes retryAction", () => {
    const retry = vi.fn();
    const err = commandError({ retryable: true, code: "database_unavailable" });

    notifyError(err, retry);

    expect(mockAddNotification).toHaveBeenCalledWith(
      expect.objectContaining({
        type: "error",
        title: "errors.database_unavailable",
        message: "Something broke",
        retryable: true,
        retryAction: retry,
        dismissAfterMs: null,
      }),
    );
  });

  test("CommandError with retryable=false omits retryAction", () => {
    const retry = vi.fn();
    const err = commandError({ retryable: false });

    notifyError(err, retry);

    expect(mockAddNotification).toHaveBeenCalledWith(
      expect.objectContaining({
        retryable: false,
        retryAction: undefined,
      }),
    );
  });

  test("incompatible provider errors show the localized CPU recovery message", () => {
    const err = commandError({
      code: "execution_provider_unavailable",
      message: "technical provider detail",
    });

    notifyError(err);

    expect(mockAddNotification).toHaveBeenCalledWith(
      expect.objectContaining({
        title: "errors.execution_provider_unavailable",
        message: "errors.executionProviderUnavailableMessage",
        retryable: false,
      }),
    );
  });

  test("regular Error produces a generic error notification", () => {
    notifyError(new Error("network timeout"));

    expect(mockT).toHaveBeenCalledWith("errors.somethingWentWrong");
    expect(mockAddNotification).toHaveBeenCalledWith(
      expect.objectContaining({
        type: "error",
        title: "errors.somethingWentWrong",
        message: "network timeout",
        retryable: false,
        dismissAfterMs: null,
      }),
    );
  });

  test("retryAction is not included when error is not retryable", () => {
    const retry = vi.fn();
    notifyError(new Error("fail"), retry);

    expect(mockAddNotification).toHaveBeenCalledWith(
      expect.objectContaining({
        retryable: false,
      }),
    );
    const call = mockAddNotification.mock.calls[0][0];
    expect(call.retryAction).toBeUndefined();
  });
});

describe("notifySuccess", () => {
  beforeEach(() => {
    mockAddNotification.mockClear();
  });

  test("passes title and message", () => {
    notifySuccess("Imported", "3 songs added");

    expect(mockAddNotification).toHaveBeenCalledWith(
      expect.objectContaining({
        type: "success",
        title: "Imported",
        message: "3 songs added",
        retryable: false,
        dismissAfterMs: 4000,
      }),
    );
  });

  test("defaults message to empty string when omitted", () => {
    notifySuccess("Done");

    expect(mockAddNotification).toHaveBeenCalledWith(
      expect.objectContaining({
        type: "success",
        title: "Done",
        message: "",
        retryable: false,
        dismissAfterMs: 4000,
      }),
    );
  });
});
