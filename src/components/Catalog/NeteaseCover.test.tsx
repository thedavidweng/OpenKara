// @vitest-environment jsdom
import { cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, describe, expect, test } from "vitest";
import { NeteaseCover } from "./NeteaseCover";

afterEach(cleanup);

describe("NeteaseCover", () => {
  test("requests a sized lazy thumbnail", () => {
    const { container } = render(
      <NeteaseCover url="https://p1.music.126.net/a.jpg" size={200} />,
    );
    const img = container.querySelector("img");
    expect(img?.getAttribute("src")).toBe(
      "https://p1.music.126.net/a.jpg?param=200y200",
    );
    expect(img?.getAttribute("loading")).toBe("lazy");
    expect(img?.getAttribute("decoding")).toBe("async");
  });

  test("falls back to the neutral tile without a url or after an error", () => {
    const { container, rerender } = render(
      <NeteaseCover url={null} size={200} />,
    );
    expect(container.querySelector("img")).toBeNull();
    rerender(<NeteaseCover url="https://p1.music.126.net/a.jpg" size={200} />);
    fireEvent.error(container.querySelector("img")!);
    expect(container.querySelector("img")).toBeNull();
  });
});
