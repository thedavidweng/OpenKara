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
import { SourceMarket } from "./SourceMarket";

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

const freshPackage: MarketPackage = {
  registry_id: "demo",
  package_id: "demo-fresh",
  source_id: "fresh-source",
  name: "Fresh Library",
  description: "A package that is not installed",
  license: "MIT",
  versions: [
    {
      version: "1.0.0",
      size: 4096,
      resource_origins: ["https://fresh.demo.test"],
      media_origins: [],
      capabilities: ["search"],
    },
  ],
  installed: null,
  update: null,
};

const webPackage: MarketPackage = {
  registry_id: "demo",
  package_id: "demo-web",
  source_id: "demo-web",
  name: "Web Library",
  description: "An installed package",
  license: "MIT",
  versions: [
    {
      version: "1.0.0",
      size: 1024,
      resource_origins: [],
      media_origins: [],
      capabilities: ["search"],
    },
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

const clonePackage: MarketPackage = {
  ...freshPackage,
  package_id: "web-clone",
  source_id: "demo-web",
  name: "Web Clone",
};

const webSnapshot: SourceSnapshot = {
  source_id: "demo-web",
  owner_binding: "registry:demo/demo-web",
  registry_id: "demo",
  package_id: "demo-web",
  name: "Web Library",
  description: "An installed package",
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

function preset(market: MarketPackage[], sources: SourceSnapshot[] = []) {
  useCatalogStore.setState({
    registries: [demoRegistry],
    registriesStatus: "ready",
    registriesError: null,
    market,
    marketStatus: "ready",
    marketError: null,
    sources,
  });
}

function renderMarket(backend = createMockBackend()) {
  return render(
    <BackendProvider backend={backend}>
      <SourceMarket />
    </BackendProvider>,
  );
}

test("lists packages with identity, license, capabilities and declared origins", () => {
  preset([freshPackage]);
  renderMarket();
  const card = screen.getByRole("article", { name: "Fresh Library" });
  expect(card.textContent).toContain("A package that is not installed");
  expect(card.textContent).toContain("MIT");
  expect(card.textContent).toContain("Demo Registry");
  expect(card.textContent).toContain("demo-fresh");
  expect(card.textContent).toContain("fresh-source");
  expect(card.textContent).toContain("1.0.0");
  expect(card.textContent).toContain("4.0 KB");
  expect(card.textContent).toContain("search");
  expect(card.textContent).toContain("https://fresh.demo.test");
});

test("install confirms the declared origins before installing", async () => {
  const installMarketPackage = vi.fn().mockResolvedValue({
    ...webSnapshot,
    source_id: "fresh-source",
    registry_id: "demo",
    package_id: "demo-fresh",
  });
  const listSourceMarket = vi.fn().mockResolvedValue([]);
  const listStreamingSources = vi.fn().mockResolvedValue([]);
  const backend = createMockBackend({
    overrides: {
      catalog: { installMarketPackage, listSourceMarket, listStreamingSources },
    },
  });
  preset([freshPackage]);
  renderMarket(backend);
  fireEvent.click(
    screen.getByRole("button", { name: "settings.streaming.market.install" }),
  );
  const dialog = screen.getByRole("dialog");
  expect(dialog.textContent).toContain("https://fresh.demo.test");
  expect(dialog.textContent).toContain("search");
  expect(installMarketPackage).not.toHaveBeenCalled();
  fireEvent.click(
    screen.getByRole("button", {
      name: "settings.streaming.market.installConfirm",
    }),
  );
  await waitFor(() =>
    expect(installMarketPackage).toHaveBeenCalledWith(
      "demo",
      "demo-fresh",
      null,
      false,
      false,
    ),
  );
  await waitFor(() => expect(listSourceMarket).toHaveBeenCalled());
  expect(screen.queryByRole("dialog")).toBeNull();
});

test("an update that widens access asks for consent and names the additions", async () => {
  const installMarketPackage = vi.fn().mockResolvedValue(webSnapshot);
  const backend = createMockBackend({
    overrides: {
      catalog: {
        installMarketPackage,
        listSourceMarket: async () => [],
        listStreamingSources: async () => [webSnapshot],
      },
    },
  });
  preset([webPackage], [webSnapshot]);
  renderMarket(backend);
  fireEvent.click(
    screen.getByRole("button", {
      name: /settings\.streaming\.market\.updateTo/,
    }),
  );
  const dialog = screen.getByRole("dialog");
  expect(dialog.textContent).toContain("https://api.demo.test");
  expect(dialog.textContent).toContain("preview");
  expect(installMarketPackage).not.toHaveBeenCalled();
  fireEvent.click(
    screen.getByRole("button", {
      name: "settings.streaming.market.consentConfirm",
    }),
  );
  await waitFor(() =>
    expect(installMarketPackage).toHaveBeenCalledWith(
      "demo",
      "demo-web",
      "1.1.0",
      true,
      false,
    ),
  );
});

test("an update without new access installs without a consent dialog", async () => {
  const installMarketPackage = vi.fn().mockResolvedValue(webSnapshot);
  const backend = createMockBackend({
    overrides: {
      catalog: {
        installMarketPackage,
        listSourceMarket: async () => [],
        listStreamingSources: async () => [webSnapshot],
      },
    },
  });
  preset(
    [
      {
        ...webPackage,
        update: {
          version: "1.1.0",
          added_origins: [],
          added_capabilities: [],
          requires_consent: false,
        },
      },
    ],
    [webSnapshot],
  );
  renderMarket(backend);
  fireEvent.click(
    screen.getByRole("button", {
      name: /settings\.streaming\.market\.updateTo/,
    }),
  );
  await waitFor(() =>
    expect(installMarketPackage).toHaveBeenCalledWith(
      "demo",
      "demo-web",
      "1.1.0",
      false,
      false,
    ),
  );
  expect(screen.queryByRole("dialog")).toBeNull();
});

test("installing a package whose source id is taken asks to replace the holder", async () => {
  const installMarketPackage = vi.fn().mockResolvedValue(webSnapshot);
  const backend = createMockBackend({
    overrides: {
      catalog: {
        installMarketPackage,
        listSourceMarket: async () => [],
        listStreamingSources: async () => [webSnapshot],
      },
    },
  });
  preset([clonePackage], [webSnapshot]);
  renderMarket(backend);
  fireEvent.click(
    screen.getByRole("button", { name: "settings.streaming.market.install" }),
  );
  const dialog = screen.getByRole("dialog");
  expect(dialog.textContent).toContain("settings.streaming.market.replaceBody");
  expect(dialog.textContent).toContain("Web Library");
  expect(installMarketPackage).not.toHaveBeenCalled();
  fireEvent.click(
    screen.getByRole("button", {
      name: "settings.streaming.market.replaceConfirm",
    }),
  );
  await waitFor(() =>
    expect(installMarketPackage).toHaveBeenCalledWith(
      "demo",
      "web-clone",
      null,
      false,
      true,
    ),
  );
});

test("a backend source_id_taken refusal opens the replacement confirmation", async () => {
  const installMarketPackage = vi.fn().mockRejectedValue({
    code: "streaming_source_unavailable",
    message: "source_id_taken: Another package provides this source.",
    retryable: false,
  });
  const backend = createMockBackend({
    overrides: { catalog: { installMarketPackage } },
  });
  // The holder is not in `sources`, so only the backend refusal can reveal it.
  preset([clonePackage], []);
  renderMarket(backend);
  fireEvent.click(
    screen.getByRole("button", { name: "settings.streaming.market.install" }),
  );
  fireEvent.click(
    screen.getByRole("button", {
      name: "settings.streaming.market.installConfirm",
    }),
  );
  await waitFor(() =>
    expect(screen.getByRole("dialog").textContent).toContain(
      "settings.streaming.market.replaceBody",
    ),
  );
});

test("a pinned install shows the pin and no update offer", () => {
  preset(
    [
      {
        ...webPackage,
        installed: { version: "1.0.0", pinned_version: "1.0.0" },
        update: null,
      },
    ],
    [webSnapshot],
  );
  renderMarket();
  const card = screen.getByRole("article", { name: "Web Library" });
  expect(card.textContent).toContain("settings.streaming.market.pinned");
  expect(
    screen.queryByRole("button", {
      name: /settings\.streaming\.market\.updateTo/,
    }),
  ).toBeNull();
});

test("install errors surface the message naming each failed download URL", async () => {
  const installMarketPackage = vi.fn().mockRejectedValue({
    code: "streaming_source_unavailable",
    message:
      "invalid_package: Every download URL failed. https://a.demo.test/pkg.tgz: connection refused. https://b.demo.test/pkg.tgz: served wrong bytes.",
    retryable: true,
  });
  const backend = createMockBackend({
    overrides: { catalog: { installMarketPackage } },
  });
  preset([freshPackage]);
  renderMarket(backend);
  fireEvent.click(
    screen.getByRole("button", { name: "settings.streaming.market.install" }),
  );
  fireEvent.click(
    screen.getByRole("button", {
      name: "settings.streaming.market.installConfirm",
    }),
  );
  await waitFor(() => {
    const alert = screen.getByRole("alert");
    expect(alert.textContent).toContain("https://a.demo.test/pkg.tgz");
    expect(alert.textContent).toContain("https://b.demo.test/pkg.tgz");
  });
});

test("without registries the market explains how to add packages", () => {
  useCatalogStore.setState({
    registries: [],
    registriesStatus: "ready",
    registriesError: null,
    market: [],
    marketStatus: "ready",
    marketError: null,
    sources: [],
  });
  renderMarket();
  expect(
    screen.getByText("settings.streaming.market.emptyNoRegistries"),
  ).toBeTruthy();
});
