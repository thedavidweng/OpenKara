import { describe, expect, it, vi } from "vitest";
import { createRuntimeEventRuntime } from "@/runtime/event-runtime";

describe("runtime event runtime", () => {
  it("starts each subscription and stops all listeners", async () => {
    const firstStop = vi.fn();
    const secondStop = vi.fn();
    const runtime = createRuntimeEventRuntime([
      { subscribe: async () => firstStop },
      { subscribe: async () => secondStop },
    ]);

    await runtime.start();
    runtime.stop();

    expect(firstStop).toHaveBeenCalledOnce();
    expect(secondStop).toHaveBeenCalledOnce();
  });

  it("unlistens a late registration after stop", async () => {
    let resolveSubscription: ((stop: () => void) => void) | undefined;
    const lateStop = vi.fn();
    const runtime = createRuntimeEventRuntime([
      {
        subscribe: () =>
          new Promise<() => void>((resolve) => {
            resolveSubscription = resolve;
          }),
      },
    ]);

    const start = runtime.start();
    runtime.stop();
    resolveSubscription?.(lateStop);
    await start;

    expect(lateStop).toHaveBeenCalledOnce();
  });

  it("can start again after stop", async () => {
    const stop = vi.fn();
    const runtime = createRuntimeEventRuntime([
      { subscribe: async () => stop },
    ]);

    await runtime.start();
    runtime.stop();
    await runtime.start();
    runtime.stop();

    expect(stop).toHaveBeenCalledTimes(2);
  });
});
