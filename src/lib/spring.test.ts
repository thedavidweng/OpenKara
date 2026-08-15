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
    expect(spring.setTarget(1)).toBe(true);
    expect(spring.isSettled()).toBe(false);
  });

  test("setTarget reports unchanged targets", () => {
    const spring = new Spring(1);
    expect(spring.setTarget(1)).toBe(false);
    expect(spring.isSettled()).toBe(true);
  });

  test("converges to target after enough updates", () => {
    const spring = new Spring(0, { stiffness: 180, damping: 12 });
    spring.setTarget(1);

    for (let i = 0; i < 120; i++) {
      spring.update(1 / 60);
    }

    expect(spring.getPosition()).toBeCloseTo(1.0, 2);
    expect(spring.isSettled()).toBe(true);
  });

  test("jumpTo immediately sets position", () => {
    const spring = new Spring(0);
    spring.setTarget(1);
    spring.update(0.016);
    spring.jumpTo(0.5);
    expect(spring.getPosition()).toBe(0.5);
    expect(spring.isSettled()).toBe(true);
  });

  test("hands off a flick velocity from the current position", () => {
    const spring = new Spring(10);
    spring.syncPosition(40);
    spring.setVelocity(200);
    spring.setTarget(120);
    expect(spring.getPosition()).toBe(40);
    expect(spring.getVelocity()).toBe(200);
    expect(spring.isSettled()).toBe(false);
  });

  test("update is a no-op when settled", () => {
    const spring = new Spring(1);
    spring.setTarget(1);
    spring.update(0.016);
    expect(spring.getPosition()).toBe(1);
  });
});
