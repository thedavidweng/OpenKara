// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { CoverArtBytes } from "@/types/ipc";
import { resetCoverArtCacheForTests, useCoverArtUrl } from "./cover-art";

function renderHook<T, P>(
  hook: (props: P) => T,
  initialProps: P,
): {
  result: { current: T };
  rerender: (props: P) => void;
  unmount: () => void;
} {
  const result = { current: undefined as unknown as T };
  let currentProps = initialProps;
  function TestComponent() {
    result.current = hook(currentProps);
    return null;
  }
  const container = document.createElement("div");
  const root = createRoot(container);
  act(() => {
    root.render(<TestComponent />);
  });
  return {
    result,
    rerender: (props: P) => {
      currentProps = props;
      act(() => {
        root.render(<TestComponent />);
      });
    },
    unmount: () => {
      act(() => root.unmount());
    },
  };
}

const createObjectURL = vi.fn(
  (blob: Blob) => `blob:${blob.type}:${Math.random().toString(36).slice(2)}`,
);
const revokeObjectURL = vi.fn();

describe("useCoverArtUrl hook lifecycle", () => {
  beforeEach(() => {
    vi.stubGlobal("URL", { createObjectURL, revokeObjectURL });
  });

  afterEach(() => {
    resetCoverArtCacheForTests();
    vi.unstubAllGlobals();
    createObjectURL.mockClear();
    revokeObjectURL.mockClear();
  });

  test("survives byte replacement across re-renders without prematurely revoking the new URL", () => {
    // Regression: when bytes change under the same songHash+size, the old
    // useEffect cleanup must not revoke the new URL. Before the fix, the
    // cleanup decremented the new entry's refs to zero and revoked it,
    // causing the ambience backdrop to silently disappear.
    const jpeg: CoverArtBytes = [0xff, 0xd8, 0x00];
    const png: CoverArtBytes = [0x89, 0x50, 0x4e, 0x47];

    const { result, rerender, unmount } = renderHook(
      (props: { bytes: CoverArtBytes }) =>
        useCoverArtUrl("song-lifecycle", props.bytes, "preview"),
      { bytes: jpeg },
    );

    const urlAfterJpeg = result.current;
    expect(urlAfterJpeg).toBeTruthy();

    // Replace bytes — simulates the async fetch delivering the real cover
    // after the hook initially rendered with a stale or placeholder set.
    rerender({ bytes: png });
    const urlAfterPng = result.current;
    expect(urlAfterPng).not.toBe(urlAfterJpeg);

    // The old URL was revoked at replacement time.
    expect(revokeObjectURL).toHaveBeenCalledWith(urlAfterJpeg);
    // The new URL must NOT have been revoked by the stale cleanup.
    expect(revokeObjectURL).not.toHaveBeenCalledWith(urlAfterPng);

    unmount();
    expect(revokeObjectURL).toHaveBeenCalledWith(urlAfterPng);
  });

  test("does not inflate ref count when bytes reference changes but content stays the same", () => {
    // Store re-emits can produce a new array reference with identical bytes.
    // The hook should not call retainCoverArtUrl again (which would increment
    // refs without a matching release), so the URL is cleaned up correctly on
    // unmount.
    const jpeg: CoverArtBytes = [0xff, 0xd8, 0x00];
    const jpegCopy: CoverArtBytes = [...jpeg];

    const { result, rerender, unmount } = renderHook(
      (props: { bytes: CoverArtBytes }) =>
        useCoverArtUrl("song-stable", props.bytes, "preview"),
      { bytes: jpeg },
    );

    const url1 = result.current;
    expect(createObjectURL).toHaveBeenCalledTimes(1);

    // New reference, same content — no new URL, no extra retain.
    rerender({ bytes: jpegCopy });
    expect(result.current).toBe(url1);
    expect(createObjectURL).toHaveBeenCalledTimes(1);

    unmount();
    expect(revokeObjectURL).toHaveBeenCalledTimes(1);
    expect(revokeObjectURL).toHaveBeenCalledWith(url1);
  });
});
