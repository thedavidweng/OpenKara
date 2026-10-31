import { describe, expect, test, vi } from "vite-plus/test";
import {
  createMockBackend,
  DEFAULT_MOCK_DATA,
} from "@/lib/backend/mock-backend";
import { createCatalogStore } from "./catalog-store";

describe("Streaming Source support", () => {
  test("a build with Streaming Sources reports them once and keeps the answer", async () => {
    const backend = createMockBackend();
    const supported = vi.spyOn(backend.catalog, "streamingSourcesSupported");
    const store = createCatalogStore(backend);

    expect(store.getState().sourcesSupported).toBeNull();
    await expect(store.getState().loadSourcesSupport()).resolves.toBe(true);
    await expect(store.getState().loadSourcesSupport()).resolves.toBe(true);

    expect(store.getState().sourcesSupported).toBe(true);
    expect(supported).toHaveBeenCalledTimes(1);
  });

  test("concurrent callers share one support check", async () => {
    const backend = createMockBackend();
    const supported = vi.spyOn(backend.catalog, "streamingSourcesSupported");
    const store = createCatalogStore(backend);

    await expect(
      Promise.all([
        store.getState().loadSourcesSupport(),
        store.getState().loadSourcesSupport(),
      ]),
    ).resolves.toEqual([true, true]);
    expect(supported).toHaveBeenCalledTimes(1);
  });

  test("a build without Streaming Sources reports them unsupported", async () => {
    const store = createCatalogStore(
      createMockBackend({
        data: { ...DEFAULT_MOCK_DATA, streamingSourcesSupported: false },
      }),
    );

    await expect(store.getState().loadSourcesSupport()).resolves.toBe(false);
    expect(store.getState().sourcesSupported).toBe(false);
  });

  test("a failed support check leaves the source surface off", async () => {
    const store = createCatalogStore(
      createMockBackend({
        overrides: {
          catalog: {
            streamingSourcesSupported: () =>
              Promise.reject(new Error("IPC unavailable")),
          },
        },
      }),
    );

    await expect(store.getState().loadSourcesSupport()).resolves.toBe(false);
    expect(store.getState().sourcesSupported).toBe(false);
  });
});
