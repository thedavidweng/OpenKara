import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";

type RomanizeRequest = {
  requestId: number;
  lines: readonly string[];
  language?: string | null;
};

class FakeWorker {
  static instances: FakeWorker[] = [];

  listeners = new Set<(event: MessageEvent) => void>();
  posted: RomanizeRequest[] = [];

  constructor(
    readonly _url: URL | string,
    readonly options?: WorkerOptions,
  ) {
    FakeWorker.instances.push(this);
  }

  addEventListener(eventType: string, listener: EventListener) {
    if (eventType === "message") {
      this.listeners.add(listener as (event: MessageEvent) => void);
    }
  }

  removeEventListener(_eventType: string, listener: EventListener) {
    this.listeners.delete(listener as (event: MessageEvent) => void);
  }

  postMessage(data: RomanizeRequest) {
    this.posted.push(data);
    const result = data.lines.map((line) =>
      line === "你好" ? "ni hao" : line,
    );
    queueMicrotask(() => {
      const event = {
        data: { requestId: data.requestId, result },
      } as MessageEvent;
      for (const listener of this.listeners) {
        listener(event);
      }
    });
  }

  terminate() {}
}

describe("romanizeLyricsLines", () => {
  const originalWorker = globalThis.Worker;

  beforeEach(() => {
    vi.resetModules();
    FakeWorker.instances = [];
    vi.stubGlobal("Worker", FakeWorker);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    if (originalWorker === undefined) {
      Reflect.deleteProperty(globalThis, "Worker");
    } else {
      globalThis.Worker = originalWorker;
    }
  });

  test("keeps Latin lyrics on the detector path without creating a worker", async () => {
    const { romanizeLyricsLines } = await import("./lyrics-romanizer");

    const { result, requestId } = await romanizeLyricsLines(["Hello world"]);
    expect(result).toEqual(["Hello world"]);
    expect(requestId).toBe(-1);
    expect(FakeWorker.instances).toHaveLength(0);
  });

  test("reuses one module worker for non-Latin lyrics", async () => {
    const { romanizeLyricsLines } = await import("./lyrics-romanizer");

    const { result } = await romanizeLyricsLines(["你好"]);
    expect(result).toEqual(["ni hao"]);
    await romanizeLyricsLines(["世界"]);

    expect(FakeWorker.instances).toHaveLength(1);
    expect(FakeWorker.instances[0]?.options).toEqual({ type: "module" });
    expect(FakeWorker.instances[0]?.posted).toHaveLength(2);
  });

  test("posts the cantonese pin to the worker", async () => {
    const { romanizeLyricsLines } = await import("./lyrics-romanizer");

    await romanizeLyricsLines(["你好"], "cantonese");

    expect(FakeWorker.instances[0]?.posted[0]).toMatchObject({
      lines: ["你好"],
      language: "cantonese",
    });
  });

  test("posts the japanese pin to the worker", async () => {
    const { romanizeLyricsLines } = await import("./lyrics-romanizer");

    await romanizeLyricsLines(["恋愛"], "japanese");

    expect(FakeWorker.instances[0]?.posted[0]).toMatchObject({
      lines: ["恋愛"],
      language: "japanese",
    });
  });

  test("posts the whole array in one message when language is unknown", async () => {
    const { romanizeLyricsLines } = await import("./lyrics-romanizer");

    const { result } = await romanizeLyricsLines(["Hello", "你好", "World"]);

    expect(result).toEqual(["Hello", "ni hao", "World"]);
    expect(FakeWorker.instances[0]?.posted).toHaveLength(1);
    expect(FakeWorker.instances[0]?.posted[0]?.lines).toEqual([
      "Hello",
      "你好",
      "World",
    ]);
  });

  test("returns monotonically increasing requestIds for non-Latin content", async () => {
    const { romanizeLyricsLines } = await import("./lyrics-romanizer");

    const { requestId: id1 } = await romanizeLyricsLines(["你好"]);
    const { requestId: id2 } = await romanizeLyricsLines(["世界"]);

    expect(id1).toBeGreaterThan(0);
    expect(id2).toBeGreaterThan(id1);
  });

  test("returns the original lines when Worker is unavailable", async () => {
    vi.stubGlobal("Worker", undefined);
    const { romanizeLyricsLines } = await import("./lyrics-romanizer");

    const { result, requestId } = await romanizeLyricsLines(["你好"]);
    expect(result).toEqual(["你好"]);
    expect(requestId).toBeGreaterThan(0);
  });
});
