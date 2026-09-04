// @vitest-environment jsdom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { beforeEach, describe, expect, test, vi } from "vitest";
import { ToastContainer } from "./ToastContainer";

const { mockNotificationState, mockCopyDebugInfo, mockNotifyError } =
  vi.hoisted(() => ({
    mockNotificationState: {
      notifications: [] as Array<{
        id: string;
        type: "error" | "warning" | "success" | "info";
        title: string;
        message: string;
        retryable: boolean;
        retryAction?: () => void;
      }>,
      dismissNotification: vi.fn(),
    },
    mockCopyDebugInfo: vi.fn().mockResolvedValue(undefined),
    mockNotifyError: vi.fn(),
  }));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
  initReactI18next: { type: "3rdParty", init: () => {} },
}));

vi.mock("@/lib/debug-info", () => ({
  copyDebugInfo: mockCopyDebugInfo,
}));

vi.mock("@/lib/errors", () => ({
  notifyError: mockNotifyError,
}));

vi.mock("@/stores/notification-store", () => ({
  useNotificationStore: (
    selector: (state: typeof mockNotificationState) => unknown,
  ) => selector(mockNotificationState),
}));

describe("ToastContainer", () => {
  beforeEach(() => {
    mockCopyDebugInfo.mockReset();
    mockCopyDebugInfo.mockResolvedValue(undefined);
    mockNotifyError.mockReset();
    mockNotificationState.dismissNotification.mockReset();
  });

  test("renders nothing when there are no notifications", () => {
    mockNotificationState.notifications = [];

    const markup = renderToStaticMarkup(<ToastContainer />);

    expect(markup).toBe("");
  });

  test("renders an error notification with the correct icon and color", () => {
    mockNotificationState.notifications = [
      {
        id: "n-1",
        type: "error",
        title: "Import Failed",
        message: "Could not read file",
        retryable: false,
      },
    ];

    const markup = renderToStaticMarkup(<ToastContainer />);

    expect(markup).toContain("Import Failed");
    expect(markup).toContain("Could not read file");
    expect(markup).toContain('role="alert"');
    expect(markup).toContain('aria-live="assertive"');
    expect(markup).toContain("text-[var(--color-destructive)]");
    expect(markup).toContain("border-[var(--color-border)]");
  });

  test("renders a success notification with green styling", () => {
    mockNotificationState.notifications = [
      {
        id: "n-2",
        type: "success",
        title: "Song Imported",
        message: "Added to library",
        retryable: false,
      },
    ];

    const markup = renderToStaticMarkup(<ToastContainer />);

    expect(markup).toContain("Song Imported");
    expect(markup).toContain('role="status"');
    expect(markup).toContain('aria-live="polite"');
    expect(markup).toContain("text-[var(--color-text)]");
    expect(markup).toContain("border-[var(--color-border)]");
  });

  test("renders a warning notification with yellow styling", () => {
    mockNotificationState.notifications = [
      {
        id: "n-3",
        type: "warning",
        title: "Slow Network",
        message: "",
        retryable: false,
      },
    ];

    const markup = renderToStaticMarkup(<ToastContainer />);

    expect(markup).toContain("Slow Network");
    expect(markup).toContain("text-[var(--color-text)]");
  });

  test("renders an info notification with blue styling", () => {
    mockNotificationState.notifications = [
      {
        id: "n-4",
        type: "info",
        title: "Tip",
        message: "Press F for fullscreen",
        retryable: false,
      },
    ];

    const markup = renderToStaticMarkup(<ToastContainer />);

    expect(markup).toContain("Tip");
    expect(markup).toContain("text-[var(--color-text)]");
  });

  test("shows a retry button when the notification is retryable", () => {
    mockNotificationState.notifications = [
      {
        id: "n-5",
        type: "error",
        title: "Separation Failed",
        message: "Model download interrupted",
        retryable: true,
        retryAction: vi.fn(),
      },
    ];

    const markup = renderToStaticMarkup(<ToastContainer />);

    expect(markup).toContain("common.tryAgain");
  });

  test("shows copy debug info on error notifications", () => {
    mockNotificationState.notifications = [
      {
        id: "n-5b",
        type: "error",
        title: "Runtime Setup Timeout",
        message: "Timed out after download",
        retryable: true,
        retryAction: vi.fn(),
      },
    ];

    const markup = renderToStaticMarkup(<ToastContainer />);

    expect(markup).toContain("settings.about.copyDebugInfo");
    expect(markup).toContain("common.tryAgain");
  });

  test("does not show a retry button for non-retryable notifications", () => {
    mockNotificationState.notifications = [
      {
        id: "n-6",
        type: "error",
        title: "Fatal Error",
        message: "Something broke",
        retryable: false,
      },
    ];

    const markup = renderToStaticMarkup(<ToastContainer />);

    expect(markup).not.toContain("common.tryAgain");
    expect(markup).toContain("settings.about.copyDebugInfo");
  });

  test("does not show copy debug info on success notifications", () => {
    mockNotificationState.notifications = [
      {
        id: "n-6b",
        type: "success",
        title: "Done",
        message: "",
        retryable: false,
      },
    ];

    const markup = renderToStaticMarkup(<ToastContainer />);

    expect(markup).not.toContain("settings.about.copyDebugInfo");
  });
  test("renders multiple notifications simultaneously", () => {
    mockNotificationState.notifications = [
      {
        id: "n-7",
        type: "success",
        title: "First",
        message: "",
        retryable: false,
      },
      {
        id: "n-8",
        type: "error",
        title: "Second",
        message: "",
        retryable: false,
      },
    ];

    const markup = renderToStaticMarkup(<ToastContainer />);

    expect(markup).toContain("First");
    expect(markup).toContain("Second");
  });

  test("renders a close button with accessible label", () => {
    mockNotificationState.notifications = [
      {
        id: "n-9",
        type: "info",
        title: "Closable",
        message: "",
        retryable: false,
      },
    ];

    const markup = renderToStaticMarkup(<ToastContainer />);

    expect(markup).toContain("common.close");
  });

  test("omits the optional message paragraph when message is empty", () => {
    mockNotificationState.notifications = [
      {
        id: "n-10",
        type: "info",
        title: "No Details",
        message: "",
        retryable: false,
      },
    ];

    const markup = renderToStaticMarkup(<ToastContainer />);

    expect(markup).toContain("No Details");
  });

  test("copies debug info when the error action is clicked", async () => {
    mockNotificationState.notifications = [
      {
        id: "n-11",
        type: "error",
        title: "Fail",
        message: "details",
        retryable: false,
      },
    ];

    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    await act(async () => {
      root.render(<ToastContainer />);
    });

    const copyButton = Array.from(container.querySelectorAll("button")).find(
      (button) => button.textContent === "settings.about.copyDebugInfo",
    );
    expect(copyButton).toBeTruthy();

    await act(async () => {
      copyButton?.click();
    });

    expect(mockCopyDebugInfo).toHaveBeenCalledOnce();
    expect(mockCopyDebugInfo).toHaveBeenCalledWith(
      expect.objectContaining({
        error: { title: "Fail", message: "details" },
      }),
    );
    expect(container.textContent).toContain("settings.about.copied");

    await act(async () => {
      root.unmount();
    });
    container.remove();
  });

  test("reports copy failures through notifyError", async () => {
    mockCopyDebugInfo.mockRejectedValueOnce(new Error("clipboard blocked"));
    mockNotificationState.notifications = [
      {
        id: "n-12",
        type: "error",
        title: "Fail",
        message: "details",
        retryable: false,
      },
    ];

    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    await act(async () => {
      root.render(<ToastContainer />);
    });

    const copyButton = Array.from(container.querySelectorAll("button")).find(
      (button) => button.textContent === "settings.about.copyDebugInfo",
    );
    expect(copyButton).toBeTruthy();

    await act(async () => {
      copyButton?.click();
    });

    expect(mockNotifyError).toHaveBeenCalledWith(expect.any(Error));

    await act(async () => {
      root.unmount();
    });
    container.remove();
  });

  test("retry action dismisses the toast after invoking the callback", async () => {
    const retryAction = vi.fn();
    mockNotificationState.notifications = [
      {
        id: "n-13",
        type: "error",
        title: "Fail",
        message: "details",
        retryable: true,
        retryAction,
      },
    ];

    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    await act(async () => {
      root.render(<ToastContainer />);
    });

    const retryButton = Array.from(container.querySelectorAll("button")).find(
      (button) => button.textContent === "common.tryAgain",
    );
    expect(retryButton).toBeTruthy();

    await act(async () => {
      retryButton?.click();
    });

    expect(retryAction).toHaveBeenCalledOnce();
    expect(mockNotificationState.dismissNotification).toHaveBeenCalledWith(
      "n-13",
    );

    await act(async () => {
      root.unmount();
    });
    container.remove();
  });
});
