// @vitest-environment jsdom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";

const { mockWindowReady } = vi.hoisted(() => ({
  mockWindowReady: vi.fn(),
}));

vi.mock("@/lib/tauri", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/tauri")>()),
  windowReady: mockWindowReady,
}));

import { useAppReadyRuntime } from "./app-runtime";

function Harness({
  scheduleFrame,
  windowShown,
  setWindowShown,
}: {
  scheduleFrame: (callback: FrameRequestCallback) => number;
  windowShown: boolean;
  setWindowShown: (shown: boolean) => void;
}) {
  useAppReadyRuntime(
    true,
    true,
    true,
    windowShown,
    setWindowShown,
    scheduleFrame,
  );
  return null;
}

describe("useAppReadyRuntime", () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;

  beforeEach(() => {
    vi.useFakeTimers();
    mockWindowReady.mockReset();
    mockWindowReady.mockResolvedValue(undefined);
    (
      globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }
    ).IS_REACT_ACT_ENVIRONMENT = true;
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
  });

  afterEach(() => {
    act(() => root.unmount());
    container.remove();
    vi.useRealTimers();
  });

  test("reveals the window even when animation frames never run", () => {
    // REGRESSION: the main window starts hidden, and macOS suspends animation
    // frames for a hidden WebView — so a reveal scheduled only through
    // requestAnimationFrame left the app running with no window at all.
    const setWindowShown = vi.fn();
    const neverFiringScheduleFrame = vi.fn(() => 1);

    act(() => {
      root.render(
        <Harness
          scheduleFrame={neverFiringScheduleFrame}
          windowShown={false}
          setWindowShown={setWindowShown}
        />,
      );
    });

    expect(neverFiringScheduleFrame).toHaveBeenCalled();
    expect(mockWindowReady).not.toHaveBeenCalled();

    act(() => {
      vi.advanceTimersByTime(120);
    });

    expect(mockWindowReady).toHaveBeenCalledTimes(1);
    expect(setWindowShown).toHaveBeenCalledWith(true);
  });

  test("requests the reveal once when the frame fires first", () => {
    const setWindowShown = vi.fn();
    const immediateScheduleFrame = vi.fn((callback: FrameRequestCallback) => {
      callback(0);
      return 1;
    });

    act(() => {
      root.render(
        <Harness
          scheduleFrame={immediateScheduleFrame}
          windowShown={false}
          setWindowShown={setWindowShown}
        />,
      );
    });

    expect(mockWindowReady).toHaveBeenCalledTimes(1);

    // The fallback timer must not fire a second request.
    act(() => {
      vi.advanceTimersByTime(500);
    });

    expect(mockWindowReady).toHaveBeenCalledTimes(1);
  });

  test("does not ask again once the window is shown", () => {
    const setWindowShown = vi.fn();

    act(() => {
      root.render(
        <Harness
          scheduleFrame={vi.fn(() => 1)}
          windowShown
          setWindowShown={setWindowShown}
        />,
      );
    });

    act(() => {
      vi.advanceTimersByTime(500);
    });

    expect(mockWindowReady).not.toHaveBeenCalled();
  });
});
