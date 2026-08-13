import { beforeEach, describe, expect, test, vi } from "vitest";

const { mockInvoke } = vi.hoisted(() => ({
  mockInvoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: mockInvoke,
}));

import { tauriBackend } from "@/lib/backend";
import * as barrel from "./tauri";

type Delegate = (...args: never[]) => unknown;

function delegatesOf(source: object): Map<string, Delegate> {
  const delegates = new Map<string, Delegate>();
  for (const [name, value] of Object.entries(source)) {
    if (typeof value === "function") {
      delegates.set(name, value as Delegate);
    }
  }
  return delegates;
}

const backendDelegates = new Map<string, Delegate>();
for (const group of Object.values(tauriBackend)) {
  for (const [name, delegate] of delegatesOf(group)) {
    backendDelegates.set(name, delegate);
  }
}

const barrelDelegates = delegatesOf(barrel);

async function commandFor(delegate: Delegate): Promise<unknown> {
  mockInvoke.mockClear();
  await delegate();
  return mockInvoke.mock.calls[0]?.[0];
}

describe("transitional tauri barrel", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
    // An array satisfies the one command that post-processes its result
    // (`getWaveform` reads `peaks.length`) and is inert for the rest.
    mockInvoke.mockResolvedValue([]);
  });

  test("re-exports every grouped backend command", () => {
    const missing = [...backendDelegates.keys()].filter(
      (name) => !barrelDelegates.has(name),
    );

    expect(missing).toEqual([]);
  });

  test.each([...backendDelegates])(
    "%s reaches the same command as the grouped backend",
    async (name, backendDelegate) => {
      const reExported = barrelDelegates.get(name);
      if (!reExported) {
        throw new Error(`${name} is not re-exported`);
      }

      const expected = await commandFor(backendDelegate);
      const actual = await commandFor(reExported);

      expect(actual).toBe(expected);
    },
  );

  test("createLibrary keeps the legacy create_library command", async () => {
    await barrel.createLibrary("/library");

    expect(mockInvoke).toHaveBeenCalledWith("create_library", {
      path: "/library",
    });
  });

  test("openLibrary keeps the legacy open_library command", async () => {
    await barrel.openLibrary("/library");

    expect(mockInvoke).toHaveBeenCalledWith("open_library", {
      path: "/library",
    });
  });

  test("getCoverArt omits size when the caller does not pass one", async () => {
    await barrel.getCoverArt("abc");

    expect(mockInvoke).toHaveBeenCalledWith("get_cover_art", { hash: "abc" });
  });

  test("getCoverArt forwards an explicit size", async () => {
    await barrel.getCoverArt("abc", "thumb");

    expect(mockInvoke).toHaveBeenCalledWith("get_cover_art", {
      hash: "abc",
      size: "thumb",
    });
  });
});
