// @vitest-environment jsdom
import { cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, describe, expect, test, vi } from "vite-plus/test";
import { StreamingCover } from "./StreamingCover";
vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: (key: string, protocol: string) =>
    `${protocol}://localhost/${key}`,
}));

afterEach(cleanup);

describe("StreamingCover", () => {
  test("loads host artwork lazily through the source-art protocol", () => {
    const { container } = render(<StreamingCover url="abc123" size={200} />);
    const img = container.querySelector("img");
    expect(img?.getAttribute("src")).toBe("source-art://localhost/abc123");
    expect(img?.getAttribute("loading")).toBe("lazy");
    expect(img?.getAttribute("decoding")).toBe("async");
  });

  test("falls back to the neutral tile without a url or after an error", () => {
    const { container, rerender } = render(
      <StreamingCover url={null} size={200} />,
    );
    expect(container.querySelector("img")).toBeNull();
    rerender(<StreamingCover url="abc123" size={200} />);
    fireEvent.error(container.querySelector("img")!);
    expect(container.querySelector("img")).toBeNull();
  });
});
