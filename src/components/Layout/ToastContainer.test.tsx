import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test, vi } from "vitest";
import { ToastContainer } from "./ToastContainer";

const { mockNotificationState } = vi.hoisted(() => ({
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
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

vi.mock("@/stores/notification-store", () => ({
  useNotificationStore: (
    selector: (state: typeof mockNotificationState) => unknown,
  ) => selector(mockNotificationState),
}));

describe("ToastContainer", () => {
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
    expect(markup).toContain("text-red-400");
    expect(markup).toContain("border-red-400/30");
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
    expect(markup).toContain("text-green-400");
    expect(markup).toContain("border-green-400/30");
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
    expect(markup).toContain("text-yellow-400");
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
    expect(markup).toContain("text-blue-400");
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
});
