import { describe, expect, test } from "vitest";
import { getContextMenuPosition } from "./context-menu-position";

describe("getContextMenuPosition", () => {
  const VIEWPORT_PADDING = 8;

  test("normal positioning when there is plenty of room", () => {
    const result = getContextMenuPosition({
      x: 200,
      y: 150,
      menuWidth: 180,
      menuHeight: 300,
      viewportWidth: 1920,
      viewportHeight: 1080,
    });

    expect(result).toEqual({ left: 200, top: 150 });
  });

  test("clamps x to VIEWPORT_PADDING when x is negative", () => {
    const result = getContextMenuPosition({
      x: -10,
      y: 100,
      menuWidth: 200,
      menuHeight: 200,
      viewportWidth: 1920,
      viewportHeight: 1080,
    });

    expect(result.left).toBe(VIEWPORT_PADDING);
  });

  test("clamps y to VIEWPORT_PADDING when y is negative", () => {
    const result = getContextMenuPosition({
      x: 100,
      y: -5,
      menuWidth: 200,
      menuHeight: 200,
      viewportWidth: 1920,
      viewportHeight: 1080,
    });

    expect(result.top).toBe(VIEWPORT_PADDING);
  });

  test("clamps x to VIEWPORT_PADDING when x is 0", () => {
    const result = getContextMenuPosition({
      x: 0,
      y: 100,
      menuWidth: 200,
      menuHeight: 200,
      viewportWidth: 1920,
      viewportHeight: 1080,
    });

    expect(result.left).toBe(VIEWPORT_PADDING);
  });

  test("clamps y to VIEWPORT_PADDING when y is 0", () => {
    const result = getContextMenuPosition({
      x: 100,
      y: 0,
      menuWidth: 200,
      menuHeight: 200,
      viewportWidth: 1920,
      viewportHeight: 1080,
    });

    expect(result.top).toBe(VIEWPORT_PADDING);
  });

  test("clamps left when menu would overflow the right edge", () => {
    const result = getContextMenuPosition({
      x: 1900,
      y: 100,
      menuWidth: 200,
      menuHeight: 200,
      viewportWidth: 1920,
      viewportHeight: 1080,
    });

    // maxLeft = Math.max(8, 1920 - 200 - 8) = Math.max(8, 1712) = 1712
    expect(result.left).toBe(1712);
  });

  test("clamps top when menu would overflow the bottom edge", () => {
    const result = getContextMenuPosition({
      x: 100,
      y: 1000,
      menuWidth: 200,
      menuHeight: 300,
      viewportWidth: 1920,
      viewportHeight: 1080,
    });

    // maxTop = Math.max(8, 1080 - 300 - 8) = Math.max(8, 772) = 772
    expect(result.top).toBe(772);
  });

  test("clamps both axes when near the bottom-right corner", () => {
    const result = getContextMenuPosition({
      x: 2000,
      y: 1100,
      menuWidth: 200,
      menuHeight: 200,
      viewportWidth: 1920,
      viewportHeight: 1080,
    });

    expect(result.left).toBe(1712);
    expect(result.top).toBe(872);
  });

  test("when menu is larger than viewport, maxLeft falls back to VIEWPORT_PADDING", () => {
    // menu is wider than viewport, so viewportWidth - menuWidth - VIEWPORT_PADDING is negative
    // maxLeft = Math.max(8, negative) = 8
    const result = getContextMenuPosition({
      x: 50,
      y: 50,
      menuWidth: 2000,
      menuHeight: 200,
      viewportWidth: 1920,
      viewportHeight: 1080,
    });

    expect(result.left).toBe(VIEWPORT_PADDING);
  });

  test("when menu is taller than viewport, maxTop falls back to VIEWPORT_PADDING", () => {
    const result = getContextMenuPosition({
      x: 50,
      y: 50,
      menuWidth: 200,
      menuHeight: 1200,
      viewportWidth: 1920,
      viewportHeight: 1080,
    });

    expect(result.top).toBe(VIEWPORT_PADDING);
  });

  test("clamps x near 0 but within padding returns the clamped value", () => {
    // x=5 < VIEWPORT_PADDING(8), so clamped to 8
    const result = getContextMenuPosition({
      x: 5,
      y: 100,
      menuWidth: 100,
      menuHeight: 100,
      viewportWidth: 1920,
      viewportHeight: 1080,
    });

    expect(result.left).toBe(VIEWPORT_PADDING);
  });

  test("exact boundary: x equals VIEWPORT_PADDING is allowed through", () => {
    const result = getContextMenuPosition({
      x: VIEWPORT_PADDING,
      y: VIEWPORT_PADDING,
      menuWidth: 100,
      menuHeight: 100,
      viewportWidth: 1920,
      viewportHeight: 1080,
    });

    expect(result).toEqual({ left: VIEWPORT_PADDING, top: VIEWPORT_PADDING });
  });
});
