// @vitest-environment jsdom
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vite-plus/test";
import { BackendProvider } from "@/lib/backend";
import { createMockBackend } from "@/lib/backend/mock-backend";
import { useCatalogStore } from "@/stores/catalog-store";
import type {
  MarketPackage,
  RegistryStatus,
  SourceSnapshot,
} from "@/types/ipc";
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

const demoRegistry: RegistryStatus = {
  url: "https://sources.demo.test/registry.json",
  registry_id: "demo",
  display_name: "Demo Registry",
  homepage: "https://sources.demo.test",
  refreshed_at: 1700000000000,
  last_error: null,
};

const registrySource: SourceSnapshot = {
  source_id: "demo-web",
  owner_binding: "registry:demo/demo-web",
  registry_id: "demo",
  package_id: "demo-web",
  name: "Web Library",
  description: "An installed registry package",
  license: "MIT",
  installed_version: "1.0.0",
  available_version: "1.1.0",
  staged_version: null,
  pinned_version: null,
  enabled: true,
  loaded: true,
  runtime_ready: true,
  running: false,
  state: "ready",
  progress: null,
  last_error: null,
  active_work: [],
  manifest: null,
};

const registryPackage: MarketPackage = {
  registry_id: "demo",
  package_id: "demo-web",
  source_id: "demo-web",
  name: "Web Library",
  description: "An installed registry package",
  license: "MIT",
  versions: [
    {
      version: "1.1.0",
      size: 2048,
      resource_origins: ["https://api.demo.test"],
      media_origins: [],
      capabilities: ["search", "preview"],
    },
  ],
  installed: { version: "1.0.0", pinned_version: null },
  update: {
    version: "1.1.0",
    added_origins: ["https://api.demo.test"],
    added_capabilities: ["preview"],
    requires_consent: true,
  },
};

function preset(overrides: {
  sources?: SourceSnapshot[];
  market?: MarketPackage[];
  registries?: RegistryStatus[];
}) {
  useCatalogStore.setState({
    sources: overrides.sources ?? [],
    sourceStates: {},
    sourcesStatus: "ready",
    sourcesError: null,
    market: overrides.market ?? [],
    marketStatus: "ready",
    marketError: null,
    registries: overrides.registries ?? [],
    registriesStatus: "ready",
    registriesError: null,
  });
}

test("a registry update that widens access asks for consent before updating", async () => {
  const installStreamingSource = vi.fn().mockResolvedValue(registrySource);
  const backend = createMockBackend({
    overrides: { catalog: { installStreamingSource } },
  });
  preset({
    sources: [registrySource],
    market: [registryPackage],
    registries: [demoRegistry],
  });
  render(
    <BackendProvider backend={backend}>
      <StreamingSources />
    </BackendProvider>,
  );
  fireEvent.click(
    screen.getByRole("button", { name: "settings.streaming.update" }),
  );
  const dialog = screen.getByRole("dialog");
  expect(dialog.textContent).toContain("https://api.demo.test");
  expect(dialog.textContent).toContain("preview");
  expect(installStreamingSource).not.toHaveBeenCalled();
  fireEvent.click(
    screen.getByRole("button", {
      name: "settings.streaming.market.consentConfirm",
    }),
  );
  await waitFor(() =>
    expect(installStreamingSource).toHaveBeenCalledWith("demo-web", null, true),
  );
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
});

test("a registry update without new access installs without consent", async () => {
  const installStreamingSource = vi.fn().mockResolvedValue(registrySource);
  const backend = createMockBackend({
    overrides: { catalog: { installStreamingSource } },
  });
  preset({
    sources: [registrySource],
    market: [
      {
        ...registryPackage,
        update: {
          version: "1.1.0",
          added_origins: [],
          added_capabilities: [],
          requires_consent: false,
        },
      },
    ],
    registries: [demoRegistry],
  });
  render(
    <BackendProvider backend={backend}>
      <StreamingSources />
    </BackendProvider>,
  );
  fireEvent.click(
    screen.getByRole("button", { name: "settings.streaming.update" }),
  );
  await waitFor(() =>
    expect(installStreamingSource).toHaveBeenCalledWith(
      "demo-web",
      null,
      false,
    ),
  );
  expect(screen.queryByRole("dialog")).toBeNull();
});

test("an installed source names its removed registry and keeps working", () => {
  preset({ sources: [registrySource], registries: [] });
  render(
    <BackendProvider backend={createMockBackend()}>
      <StreamingSources />
    </BackendProvider>,
  );
  expect(screen.getByText("settings.streaming.registryRemoved")).toBeTruthy();
});

test("an installed source with a listed registry shows no removal note", () => {
  preset({ sources: [registrySource], registries: [demoRegistry] });
  render(
    <BackendProvider backend={createMockBackend()}>
      <StreamingSources />
    </BackendProvider>,
  );
  expect(screen.queryByText("settings.streaming.registryRemoved")).toBeNull();
});
