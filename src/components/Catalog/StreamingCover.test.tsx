// @vitest-environment jsdom
import { cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, describe, expect, test, vi } from "vitest";
import { StreamingCover } from "./StreamingCover";
vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: (path: string) => `asset://localhost${path}`,
}));

afterEach(cleanup);

describe("StreamingCover", () => {
  test("loads the host-managed asset lazily without modifying its path", () => {
    const { container } = render(
      <StreamingCover url="/source-artwork/alpha/cover.img" size={200} />,
    );
    const img = container.querySelector("img");
    expect(img?.getAttribute("src")).toBe(
      "asset://localhost/source-artwork/alpha/cover.img",
    );
    expect(img?.getAttribute("loading")).toBe("lazy");
    expect(img?.getAttribute("decoding")).toBe("async");
  });

  test("falls back to the neutral tile without a url or after an error", () => {
    const { container, rerender } = render(
      <StreamingCover url={null} size={200} />,
    );
    expect(container.querySelector("img")).toBeNull();
    rerender(
      <StreamingCover url="/source-artwork/alpha/cover.img" size={200} />,
    );
    fireEvent.error(container.querySelector("img")!);
    expect(container.querySelector("img")).toBeNull();
  });
});
