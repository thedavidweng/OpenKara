import { describe, expect, test } from "vitest";
import { Spring } from "./spring";

describe("Spring", () => {
  test("starts at initial value", () => {
    const spring = new Spring(0.5);
    expect(spring.getPosition()).toBe(0.5);
    expect(spring.isSettled()).toBe(true);
  });

  test("setTarget starts animation", () => {
    const spring = new Spring(0);
    spring.setTarget(1);
    expect(spring.isSettled()).toBe(false);
  });

  test("converges to target after enough updates", () => {
    const spring = new Spring(0, { stiffness: 180, damping: 12 });
    spring.setTarget(1);

    // Simulate 2 seconds at 60fps
    for (let i = 0; i < 120; i++) {
      spring.update(1 / 60);
    }

    expect(spring.getPosition()).toBeCloseTo(1.0, 2);
    expect(spring.isSettled()).toBe(true);
  });

  test("jumpTo immediately sets position", () => {
    const spring = new Spring(0);
    spring.setTarget(1);
    spring.update(0.016); // advance a bit
    spring.jumpTo(0.5);
    expect(spring.getPosition()).toBe(0.5);
    expect(spring.isSettled()).toBe(true);
  });

  test("update is a no-op when settled", () => {
    const spring = new Spring(1);
    spring.setTarget(1);
    spring.update(0.016);
    expect(spring.getPosition()).toBe(1);
  });
});
