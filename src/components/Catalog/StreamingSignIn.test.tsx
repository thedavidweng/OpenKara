// @vitest-environment jsdom
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { createMockBackend } from "@/lib/backend/mock-backend";
import {
  createCatalogStore,
  type CatalogStore,
  type CatalogState,
} from "@/stores/catalog-store";
import type { SourceSnapshot } from "@/types/ipc";
import { StreamingSignIn } from "./StreamingSignIn";
const shared = vi.hoisted(() => ({ store: null as CatalogStore | null }));
vi.mock("@/stores/catalog-store", async (original) => ({
  ...(await original<{ createCatalogStore: typeof createCatalogStore }>()),
  useCatalogStore: (selector: (state: CatalogState) => unknown) =>
    shared.store!(selector),
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
  initReactI18next: { type: "3rdParty", init: () => {} },
}));
const source: SourceSnapshot = {
  source_id: "alpha",
  owner_binding: "test/alpha",
  name: "Alpha",
  description: "Fixture source",
  license: "MIT",
  installed_version: "1",
  available_version: "1",
  staged_version: null,
  pinned_version: null,
  enabled: true,
  runtime_ready: true,
  running: false,
  state: "ready",
  progress: null,
  last_error: null,
  active_work: [],
  manifest: null,
};
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});
beforeEach(() => vi.useFakeTimers());

test("plugin text is data, completion is verified by polling, and source switching cannot redirect the result", async () => {
  const backend = createMockBackend({
    overrides: {
      catalog: {
        openStreamingSignIn: async (source_id) => ({
          source_id,
          challenge: "challenge-alpha",
          expires_at: "2099-01-01T00:00:00Z",
          poll_after_ms: 250,
          presentation: {
            kind: "text",
            text: "<script>never execute</script>",
          },
        }),
        pollStreamingSignIn: async () => ({
          state: "complete",
          session: {
            source_id: "alpha",
            signed_in: true,
            display_name: "Alice",
            expired: false,
          },
        }),
      },
    },
  });
  const store = createCatalogStore(backend);
  shared.store = store;
  store
    .getState()
    .applySources([source, { ...source, source_id: "beta", name: "Beta" }]);
  store.getState().setActiveSource("alpha");
  render(<StreamingSignIn sourceId="alpha" />);
  await act(async () =>
    fireEvent.click(
      screen.getByRole("button", { name: "catalog.streaming.connect" }),
    ),
  );
  expect(screen.getByText("<script>never execute</script>")).toBeTruthy();
  expect(document.querySelector("script")).toBeNull();
  act(() => store.getState().setActiveSource("beta"));
  await act(async () => vi.advanceTimersByTimeAsync(250));
  act(() => store.getState().setActiveSource("alpha"));
  expect(screen.queryByText("<script>never execute</script>")).toBeNull();
  expect(screen.getByText("Alice")).toBeTruthy();
});

test("QR challenges render a local QR code and cancel without opening plugin-supplied markup", async () => {
  const cancel = vi.fn().mockResolvedValue(undefined);
  const store = createCatalogStore(
    createMockBackend({
      overrides: {
        catalog: {
          openStreamingSignIn: async (source_id) => ({
            source_id,
            challenge: "qr-alpha",
            expires_at: "2099-01-01T00:00:00Z",
            poll_after_ms: 1000,
            presentation: {
              kind: "qr",
              content: "https://example.invalid/auth?id=opaque",
              text: "Scan with the service app",
            },
          }),
          cancelStreamingSignIn: cancel,
        },
      },
    }),
  );
  shared.store = store;
  store.getState().applySources([source]);
  store.getState().setActiveSource("alpha");
  render(<StreamingSignIn />);
  await act(async () =>
    fireEvent.click(
      screen.getByRole("button", { name: "catalog.streaming.connect" }),
    ),
  );
  expect(screen.getByTitle("catalog.streaming.scanCode").tagName).toBe("title");
  expect(document.querySelector("img")).toBeNull();
  await act(async () =>
    fireEvent.click(screen.getByRole("button", { name: "common.cancel" })),
  );
  expect(cancel).toHaveBeenCalledWith("alpha", "qr-alpha");
  expect(screen.queryByText("Scan with the service app")).toBeNull();
});

test("web authentication presents the controlled-window status and accepts only its original source completion", async () => {
  const store = createCatalogStore(
    createMockBackend({
      overrides: {
        catalog: {
          openStreamingSignIn: async (source_id) => ({
            source_id,
            challenge: "web-alpha",
            expires_at: "2099-01-01T00:00:00Z",
            poll_after_ms: 1000,
            presentation: { kind: "web" },
          }),
        },
      },
    }),
  );
  shared.store = store;
  store.getState().applySources([source]);
  store.getState().setActiveSource("alpha");
  render(<StreamingSignIn sourceId="alpha" />);
  await act(async () =>
    fireEvent.click(
      screen.getByRole("button", { name: "catalog.streaming.connect" }),
    ),
  );
  expect(screen.getByText("catalog.streaming.connectHintWindow")).toBeTruthy();
  act(() =>
    store.getState().acceptSignIn({
      source_id: "other",
      signed_in: true,
      display_name: "Wrong account",
      expired: false,
    }),
  );
  expect(screen.queryByText("Wrong account")).toBeNull();
  act(() =>
    store.getState().acceptSignIn({
      source_id: "alpha",
      signed_in: true,
      display_name: "Verified Alice",
      expired: false,
    }),
  );
  expect(screen.getByText("Verified Alice")).toBeTruthy();
});

test("late authentication completion cannot restore a disabled source", async () => {
  let resolve!: (result: {
    state: "complete";
    session: {
      source_id: string;
      signed_in: boolean;
      display_name: string;
      expired: boolean;
    };
  }) => void;
  const promise = new Promise<{
    state: "complete";
    session: {
      source_id: string;
      signed_in: boolean;
      display_name: string;
      expired: boolean;
    };
  }>((complete) => {
    resolve = complete;
  });
  const store = createCatalogStore(
    createMockBackend({
      overrides: {
        catalog: {
          openStreamingSignIn: async (source_id) => ({
            source_id,
            challenge: "late-alpha",
            expires_at: "2099-01-01T00:00:00Z",
            poll_after_ms: 250,
            presentation: { kind: "text", text: "Waiting" },
          }),
          pollStreamingSignIn: () => promise,
        },
      },
    }),
  );
  shared.store = store;
  store.getState().applySources([source]);
  store.getState().setActiveSource("alpha");
  render(<StreamingSignIn sourceId="alpha" />);
  await act(async () =>
    fireEvent.click(
      screen.getByRole("button", { name: "catalog.streaming.connect" }),
    ),
  );
  await act(async () => vi.advanceTimersByTimeAsync(250));
  act(() =>
    store
      .getState()
      .applySource({ ...source, enabled: false, state: "disabled" }),
  );
  await act(async () =>
    resolve({
      state: "complete",
      session: {
        source_id: "alpha",
        signed_in: true,
        display_name: "Late account",
        expired: false,
      },
    }),
  );
  expect(screen.queryByText("Late account")).toBeNull();
});

test("an older web challenge error cannot dismiss the current challenge", async () => {
  let sequence = 0;
  const store = createCatalogStore(
    createMockBackend({
      overrides: {
        catalog: {
          openStreamingSignIn: async (source_id) => ({
            source_id,
            challenge: `challenge-${++sequence}`,
            expires_at: "2099-01-01T00:00:00Z",
            poll_after_ms: 1000,
            presentation: { kind: "text", text: `Prompt ${sequence}` },
          }),
          cancelStreamingSignIn: async () => {},
        },
      },
    }),
  );
  shared.store = store;
  store.getState().applySources([source]);
  store.getState().setActiveSource("alpha");
  render(<StreamingSignIn />);
  await act(async () => {
    await store.getState().openSignIn();
    await store.getState().openSignIn();
  });
  act(() =>
    store.getState().setSignInError("alpha", "challenge-1", {
      code: "streaming_auth_failed",
      message: "Old window closed",
      retryable: true,
      fallback: "retry",
    }),
  );
  expect(screen.getByText("Prompt 2")).toBeTruthy();
  expect(screen.queryByText(/Old window closed/)).toBeNull();
});

test("an older cancellation response cannot remove a replacement prompt", async () => {
  let finishOld!: () => void;
  const delayed = new Promise<void>((resolve) => {
    finishOld = resolve;
  });
  let cancellations = 0;
  let sequence = 0;
  const store = createCatalogStore(
    createMockBackend({
      overrides: {
        catalog: {
          openStreamingSignIn: async (source_id) => ({
            source_id,
            challenge: `challenge-${++sequence}`,
            expires_at: "2099-01-01T00:00:00Z",
            poll_after_ms: 1000,
            presentation: { kind: "text", text: `Prompt ${sequence}` },
          }),
          cancelStreamingSignIn: () =>
            ++cancellations === 1 ? delayed : Promise.resolve(),
        },
      },
    }),
  );
  shared.store = store;
  store.getState().applySources([source]);
  store.getState().setActiveSource("alpha");
  render(<StreamingSignIn />);
  await act(async () => store.getState().openSignIn());
  let cancellation!: Promise<void>;
  act(() => {
    cancellation = store.getState().cancelSignIn();
  });
  await act(async () => store.getState().openSignIn());
  await act(async () => {
    finishOld();
    await cancellation;
  });
  expect(screen.getByText("Prompt 2")).toBeTruthy();
});
