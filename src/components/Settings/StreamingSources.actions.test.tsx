// @vitest-environment jsdom
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vite-plus/test";
import { BackendProvider } from "@/lib/backend";
import { createMockBackend } from "@/lib/backend/mock-backend";
import {
  useCatalogStore,
  type SourceBrowseState,
} from "@/stores/catalog-store";
import type { MarketPackage, SourceSnapshot } from "@/types/ipc";
import { StreamingSources } from "./StreamingSources";

vi.mock("react-i18next", () => ({
  initReactI18next: { type: "3rdParty", init: () => {} },
  useTranslation: () => ({
    t: (key: string, params?: Record<string, unknown>) =>
      params ? `${key} ${JSON.stringify(params)}` : key,
    i18n: { language: "en" },
  }),
}));

afterEach(cleanup);

const snapshot = (id: string, patch: Partial<SourceSnapshot> = {}) =>
  ({
    source_id: id,
    owner_binding: `registry:official/${id}`,
    registry_id: "official",
    package_id: id,
    name: id,
    description: "",
    license: "MIT",
    installed_version: "1.0.0",
    available_version: "1.1.0",
    staged_version: null,
    pinned_version: null,
    enabled: true,
    loaded: true,
    running: false,
    state: "ready",
    progress: null,
    last_error: null,
    active_work: [],
    manifest: null,
    ...patch,
  }) satisfies SourceSnapshot;

const listing = (id: string): MarketPackage => ({
  registry_id: "official",
  package_id: id,
  source_id: id,
  name: id,
  description: "",
  license: "MIT",
  versions: [],
  installed: { version: "1.0.0", pinned_version: null },
  update: {
    version: "1.1.0",
    added_origins: [],
    added_capabilities: [],
    requires_consent: false,
  },
  installable_version: "1.1.0",
  required_host_version: null,
});

function setup(sources: SourceSnapshot[], market: MarketPackage[] = []) {
  const catalog = {
    installStreamingSource: vi.fn(async (id: string) => snapshot(id)),
    setStreamingSourceEnabled: vi.fn(async (id: string, enabled: boolean) =>
      snapshot(id, { enabled }),
    ),
    pinStreamingSource: vi.fn(async (id: string) =>
      snapshot(id, { pinned_version: "1.0.0" }),
    ),
    cancelStreamingSourceInstall: vi.fn(async () => {}),
    listSourceMarket: vi.fn(async () => market),
  };
  useCatalogStore.setState({
    sources,
    sourcesStatus: "ready",
    sourcesError: null,
    registries: [
      {
        url: "https://official.test/registry.json",
        registry_id: "official",
        display_name: "Official",
        homepage: "https://official.test",
        refreshed_at: null,
        last_error: null,
        official: true,
      },
    ],
    registriesStatus: "ready",
    market,
    marketStatus: "ready",
  });
  render(
    <BackendProvider backend={createMockBackend({ overrides: { catalog } })}>
      <StreamingSources />
    </BackendProvider>,
  );
  return catalog;
}

test("update all installs every update that needs no consent", async () => {
  const catalog = setup(
    [snapshot("one"), snapshot("two")],
    [listing("one"), listing("two")],
  );
  fireEvent.click(
    screen.getByRole("button", { name: /settings.streaming.updateAll/ }),
  );
  await waitFor(() =>
    expect(catalog.installStreamingSource).toHaveBeenCalledTimes(2),
  );
  expect(catalog.installStreamingSource).toHaveBeenCalledWith(
    "one",
    null,
    false,
  );
  expect(catalog.installStreamingSource).toHaveBeenCalledWith(
    "two",
    null,
    false,
  );
  await waitFor(() => expect(catalog.listSourceMarket).toHaveBeenCalled());
});

test("enable, disable and pin call the host directly", async () => {
  const catalog = setup([snapshot("on"), snapshot("off", { enabled: false })]);
  const [disable] = screen.getAllByRole("button", {
    name: "settings.streaming.disable",
  });
  fireEvent.click(disable);
  await waitFor(() =>
    expect(catalog.setStreamingSourceEnabled).toHaveBeenCalledWith(
      "on",
      false,
      false,
    ),
  );
  fireEvent.click(
    within(screen.getByRole("article", { name: "off" })).getByRole("button", {
      name: "settings.streaming.enable",
    }),
  );
  await waitFor(() =>
    expect(catalog.setStreamingSourceEnabled).toHaveBeenCalledWith(
      "off",
      true,
      false,
    ),
  );
  fireEvent.click(
    screen.getAllByRole("button", { name: "settings.streaming.pin" })[0],
  );
  await waitFor(() =>
    expect(catalog.pinStreamingSource).toHaveBeenCalledWith("on", "1.0.0"),
  );
});

test("an active install can be cancelled", async () => {
  const catalog = setup([snapshot("busy", { state: "installing" })]);
  fireEvent.click(screen.getByRole("button", { name: "common.cancel" }));
  await waitFor(() =>
    expect(catalog.cancelStreamingSourceInstall).toHaveBeenCalledWith("busy"),
  );
});

test("check for updates refreshes sources, registries and the market", async () => {
  setup([snapshot("one")]);
  const loadSources = vi.fn(async () => {});
  const loadRegistries = vi.fn(async () => {});
  const loadMarket = vi.fn(async () => {});
  useCatalogStore.setState({ loadSources, loadRegistries, loadMarket });
  fireEvent.click(
    screen.getByRole("button", { name: "settings.streaming.refresh" }),
  );
  await waitFor(() => expect(loadMarket).toHaveBeenCalled());
  expect(loadSources).toHaveBeenCalledWith(true);
  expect(loadRegistries).toHaveBeenCalled();
});

test("rows summarize state, account, staged and pinned versions", () => {
  useCatalogStore.setState({
    sourceStates: {
      expired: {
        session: {
          source_id: "expired",
          signed_in: true,
          display_name: null,
          expired: true,
        },
        errors: {},
      } as SourceBrowseState,
      named: {
        session: {
          source_id: "named",
          signed_in: true,
          display_name: "Ana",
          expired: false,
        },
        errors: {},
      } as SourceBrowseState,
    },
  });
  setup([
    snapshot("expired", { staged_version: "1.1.0" }),
    snapshot("named", { pinned_version: "1.0.0", loaded: false }),
    snapshot("plain", { available_version: null }),
  ]);
  const expired = screen.getByRole("article", { name: "expired" });
  expect(expired.textContent).toContain("catalog.streaming.sessionExpired");
  expect(expired.textContent).toContain("settings.streaming.staged");
  const named = screen.getByRole("article", { name: "named" });
  expect(named.textContent).toContain("settings.streaming.signedInAs");
  expect(named.textContent).toContain("settings.streaming.unpin · 1.0.0");
  expect(named.textContent).toContain("settings.streaming.loadedNotReady");
  expect(screen.getByRole("article", { name: "plain" }).textContent).toContain(
    "—",
  );
});

test("a failed source shows retry, and a failed action shows its message", async () => {
  const catalog = setup([
    snapshot("broken", {
      last_error: { code: "network", message: "offline", retryable: true },
    }),
    snapshot("new", { installed_version: null, state: "update_required" }),
  ]);
  catalog.installStreamingSource.mockRejectedValueOnce(new Error("refused"));
  fireEvent.click(screen.getByRole("button", { name: "common.retry" }));
  await waitFor(() =>
    expect(
      screen.getByRole("article", { name: "broken" }).textContent,
    ).toContain("refused"),
  );
  expect(
    within(screen.getByRole("article", { name: "new" })).getByRole("button", {
      name: "settings.streaming.install",
    }),
  ).toBeTruthy();
});
