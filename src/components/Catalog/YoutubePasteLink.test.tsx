// @vitest-environment jsdom
import userEvent from "@testing-library/user-event";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { YoutubePasteLink } from "./YoutubePasteLink";

const {
  mockResolve,
  mockRemember,
  mockAddToQueue,
  mockPlayNow,
  mockNotifyError,
} = vi.hoisted(() => ({
  mockResolve: vi.fn(),
  mockRemember: vi.fn(),
  mockAddToQueue: vi.fn(),
  mockPlayNow: vi.fn(),
  mockNotifyError: vi.fn(),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock("@/lib/backend", () => ({
  useBackend: () => ({
    catalog: { resolveVideoSourceUrl: mockResolve },
  }),
}));

vi.mock("@/lib/errors", () => ({
  notifyError: mockNotifyError,
}));

vi.mock("@/stores/catalog-store", () => ({
  useCatalogStore: (
    selector: (state: { rememberVideoItems: typeof mockRemember }) => unknown,
  ) => selector({ rememberVideoItems: mockRemember }),
}));

vi.mock("@/stores/queue-store", () => ({
  useQueueStore: (
    selector: (state: { addToQueue: typeof mockAddToQueue }) => unknown,
  ) => selector({ addToQueue: mockAddToQueue }),
}));

vi.mock("@/stores/player-store", () => ({
  usePlayerStore: (
    selector: (state: { playNow: typeof mockPlayNow }) => unknown,
  ) => selector({ playNow: mockPlayNow }),
}));

describe("YoutubePasteLink", () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    (
      globalThis as typeof globalThis & {
        IS_REACT_ACT_ENVIRONMENT?: boolean;
      }
    ).IS_REACT_ACT_ENVIRONMENT = true;
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
    user = userEvent.setup();
    mockResolve.mockReset();
    mockRemember.mockReset();
    mockAddToQueue.mockReset();
    mockPlayNow.mockReset();
    mockNotifyError.mockReset();
  });

  afterEach(() => {
    act(() => root.unmount());
    container.remove();
  });

  test("queues a resolved watch link and can play it now", async () => {
    mockResolve.mockResolvedValue([
      { id: "yt:one", title: "One" },
      { id: "yt:two", title: "Two" },
    ]);
    await act(async () => {
      root.render(<YoutubePasteLink />);
    });
    const input = container.querySelector("input") as HTMLInputElement;
    await user.type(input, "https://www.youtube.com/watch?v=dQw4w9WgXcQ");
    await user.click(
      Array.from(container.querySelectorAll("button")).find((button) =>
        button.textContent?.includes("youtube.addToQueue"),
      )!,
    );
    expect(mockResolve).toHaveBeenCalledWith(
      "youtube",
      "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
    );
    expect(mockRemember).toHaveBeenCalled();
    expect(mockAddToQueue).toHaveBeenCalledWith("yt:one");
    expect(mockAddToQueue).toHaveBeenCalledWith("yt:two");
    expect(mockPlayNow).not.toHaveBeenCalled();

    await user.type(input, "https://www.youtube.com/watch?v=dQw4w9WgXcQ");
    await user.click(
      Array.from(container.querySelectorAll("button")).find((button) =>
        button.textContent?.includes("library.playNow"),
      )!,
    );
    expect(mockPlayNow).toHaveBeenCalledWith("yt:one");
    expect(mockAddToQueue).toHaveBeenCalledWith("yt:two");
  });

  test("ignores an empty paste and reports a resolve failure", async () => {
    mockResolve.mockRejectedValue(new Error("private"));
    await act(async () => {
      root.render(<YoutubePasteLink />);
    });
    await user.click(
      Array.from(container.querySelectorAll("button")).find((button) =>
        button.textContent?.includes("youtube.addToQueue"),
      )!,
    );
    expect(mockResolve).not.toHaveBeenCalled();

    const input = container.querySelector("input") as HTMLInputElement;
    await user.type(input, "https://www.youtube.com/watch?v=nope");
    await user.click(
      Array.from(container.querySelectorAll("button")).find((button) =>
        button.textContent?.includes("youtube.addToQueue"),
      )!,
    );
    expect(mockNotifyError).toHaveBeenCalled();
  });
});
