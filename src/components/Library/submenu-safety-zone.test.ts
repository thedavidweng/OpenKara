import { describe, expect, test } from "vitest";
import { pointInConvexPolygon, isInSafetyZone } from "./submenu-safety-zone";
import type { Point } from "./submenu-safety-zone";

describe("pointInConvexPolygon", () => {
  const square: Point[] = [
    { x: 0, y: 0 },
    { x: 10, y: 0 },
    { x: 10, y: 10 },
    { x: 0, y: 10 },
  ];

  test("point inside a square returns true", () => {
    expect(pointInConvexPolygon({ x: 5, y: 5 }, square)).toBe(true);
  });

  test("point outside a square returns false", () => {
    expect(pointInConvexPolygon({ x: 15, y: 5 }, square)).toBe(false);
  });

  test("point outside above the square returns false", () => {
    expect(pointInConvexPolygon({ x: 5, y: -1 }, square)).toBe(false);
  });

  test("point outside below the square returns false", () => {
    expect(pointInConvexPolygon({ x: 5, y: 15 }, square)).toBe(false);
  });

  test("point on an edge (top) is considered inside (cross=0)", () => {
    expect(pointInConvexPolygon({ x: 5, y: 0 }, square)).toBe(true);
  });

  test("point on an edge (right) is considered inside", () => {
    expect(pointInConvexPolygon({ x: 10, y: 5 }, square)).toBe(true);
  });

  test("point exactly at a vertex is considered inside", () => {
    expect(pointInConvexPolygon({ x: 0, y: 0 }, square)).toBe(true);
  });

  test("returns false with fewer than 3 vertices (0 vertices)", () => {
    expect(pointInConvexPolygon({ x: 5, y: 5 }, [])).toBe(false);
  });

  test("returns false with fewer than 3 vertices (1 vertex)", () => {
    expect(pointInConvexPolygon({ x: 5, y: 5 }, [{ x: 0, y: 0 }])).toBe(false);
  });

  test("returns false with fewer than 3 vertices (2 vertices)", () => {
    expect(
      pointInConvexPolygon({ x: 5, y: 5 }, [
        { x: 0, y: 0 },
        { x: 10, y: 0 },
      ]),
    ).toBe(false);
  });

  test("works with a triangle", () => {
    const triangle: Point[] = [
      { x: 0, y: 0 },
      { x: 10, y: 0 },
      { x: 5, y: 10 },
    ];

    expect(pointInConvexPolygon({ x: 5, y: 3 }, triangle)).toBe(true);
    expect(pointInConvexPolygon({ x: 1, y: 8 }, triangle)).toBe(false);
  });
});

describe("isInSafetyZone", () => {
  // The safety zone is the trapezoid formed between the right edge of
  // parentRect and the left edge of submenuRect.
  //
  // parentRect: right=200, top=50, bottom=150
  // submenuRect: left=210, top=30, bottom=170
  //
  // Trapezoid vertices:
  //   (200, 50)  (200, 150)  (210, 170)  (210, 30)

  const parentRect: DOMRect = {
    x: 100,
    y: 50,
    width: 100,
    height: 100,
    left: 100,
    top: 50,
    right: 200,
    bottom: 150,
  } as DOMRect;
  const submenuRect: DOMRect = {
    x: 210,
    y: 30,
    width: 120,
    height: 140,
    left: 210,
    top: 30,
    right: 330,
    bottom: 170,
  } as DOMRect;

  test("point inside the trapezoid returns true", () => {
    // (205, 100) is between right edge of parent (200) and left edge of submenu (210)
    // and between the top and bottom boundaries
    expect(isInSafetyZone({ x: 205, y: 100 }, parentRect, submenuRect)).toBe(
      true,
    );
  });

  test("point exactly on the parent right edge is inside", () => {
    expect(isInSafetyZone({ x: 200, y: 100 }, parentRect, submenuRect)).toBe(
      true,
    );
  });

  test("point exactly on the submenu left edge is inside", () => {
    expect(isInSafetyZone({ x: 210, y: 100 }, parentRect, submenuRect)).toBe(
      true,
    );
  });

  test("point to the left of the parent right edge returns false", () => {
    expect(isInSafetyZone({ x: 190, y: 100 }, parentRect, submenuRect)).toBe(
      false,
    );
  });

  test("point to the right of the submenu left edge returns false", () => {
    expect(isInSafetyZone({ x: 220, y: 100 }, parentRect, submenuRect)).toBe(
      false,
    );
  });

  test("point above the trapezoid returns false", () => {
    // y=25 is above both parentRect.top=50 and submenuRect.top=30
    expect(isInSafetyZone({ x: 205, y: 25 }, parentRect, submenuRect)).toBe(
      false,
    );
  });

  test("point below the trapezoid returns false", () => {
    // y=175 is below both parentRect.bottom=150 and submenuRect.bottom=170
    expect(isInSafetyZone({ x: 205, y: 175 }, parentRect, submenuRect)).toBe(
      false,
    );
  });

  test("point inside the submenu area but not in the gap returns false", () => {
    // x=250 is well inside the submenu, not in the gap
    expect(isInSafetyZone({ x: 250, y: 100 }, parentRect, submenuRect)).toBe(
      false,
    );
  });
});
