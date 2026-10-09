// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { LibraryDecisionDialog } from "./LibraryDecisionDialog";

const mockResolve = vi.fn();
let currentPending: typeof pending | null = null;
const pending = {
  remote_track_id: "1",
  library: {
    title: "Old",
    artist: "A",
    album: "Album",
    format: "MP3",
    bit_rate_bps: 192000,
    duration_ms: 1000,
    file_size_bytes: 100,
  },
  incoming: {
    title: "New",
    artist: "A",
    album: null,
    format: "FLAC",
    bit_rate_bps: null,
    duration_ms: null,
    file_size_bytes: 200,
  },
};

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock("@/hooks/use-modal-dialog", () => ({
  useModalDialog: () => {},
}));

vi.mock("@/components/Overlay/DialogBackdrop", () => ({
  DialogBackdrop: ({
    onDismiss,
    ariaLabel,
  }: {
    onDismiss: () => void;
    ariaLabel: string;
  }) => (
    <button type="button" aria-label={ariaLabel} onClick={onDismiss}>
      close
    </button>
  ),
}));

vi.mock("@/stores/catalog-store", () => ({
  useCatalogStore: (
    selector: (state: {
      pendingConflict: typeof pending | null;
      resolveConflict: typeof mockResolve;
    }) => unknown,
  ) =>
    selector({
      pendingConflict: currentPending,
      resolveConflict: mockResolve,
    }),
}));

describe("LibraryDecisionDialog", () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;

  beforeEach(() => {
    (
      globalThis as typeof globalThis & {
        IS_REACT_ACT_ENVIRONMENT?: boolean;
      }
    ).IS_REACT_ACT_ENVIRONMENT = true;
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
    mockResolve.mockReset();
    currentPending = pending;
  });

  test("renders nothing when there is no pending Library Decision", async () => {
    currentPending = null;
    await act(async () => {
      root.render(<LibraryDecisionDialog />);
    });
    expect(container.querySelector("[role='dialog']")).toBeNull();
  });

  afterEach(() => {
    act(() => root.unmount());
    container.remove();
  });

  test("offers Keep, Replace, and Apply to Remaining", async () => {
    await act(async () => {
      root.render(<LibraryDecisionDialog />);
    });
    expect(container.textContent).toContain("library.decision.title");
    expect(container.textContent).toContain("Old");
    expect(container.textContent).toContain("New");

    const buttons = Array.from(container.querySelectorAll("button"));
    buttons
      .find((button) => button.textContent?.includes("library.decision.keep"))
      ?.click();
    buttons
      .find((button) =>
        button.textContent?.includes("library.decision.replace"),
      )
      ?.click();
    buttons
      .find((button) =>
        button.textContent?.includes("library.decision.applyToRemaining"),
      )
      ?.click();
    buttons
      .find((button) => button.getAttribute("aria-label") === "common.close")
      ?.click();

    expect(mockResolve).toHaveBeenCalledWith("keep");
    expect(mockResolve).toHaveBeenCalledWith("replace");
    expect(mockResolve).toHaveBeenCalledWith("apply_replace");
    expect(mockResolve).toHaveBeenCalledWith("cancel");
  });
});
