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
import type { RegistryStatus } from "@/types/ipc";
import { SourceRegistries } from "./SourceRegistries";

vi.mock("react-i18next", () => ({
  initReactI18next: { type: "3rdParty", init: () => {} },
  useTranslation: () => ({
    t: (key: string, params?: Record<string, unknown>) =>
      params ? `${key} ${JSON.stringify(params)}` : key,
    i18n: { language: "en" },
  }),
}));

afterEach(cleanup);

const registry: RegistryStatus = {
  url: "https://sources.example.com/registry.json",
  registry_id: "example",
  display_name: "Example Registry",
  homepage: "https://sources.example.com",
  refreshed_at: 1700000000000,
  last_error: null,
};

function preset(registries: RegistryStatus[]) {
  useCatalogStore.setState({
    registries,
    registriesStatus: "ready",
    registriesError: null,
    market: [],
    marketStatus: "ready",
    marketError: null,
  });
}

test("lists registries with the last refresh time and fetch errors", () => {
  preset([
    registry,
    {
      ...registry,
      registry_id: "broken",
      display_name: "Broken Registry",
      url: "https://broken.example.com/registry.json",
      homepage: "https://broken.example.com",
      refreshed_at: null,
      last_error: {
        code: "network",
        message: "network: connection refused",
        retryable: true,
      },
    },
  ]);
  render(
    <BackendProvider backend={createMockBackend()}>
      <SourceRegistries />
    </BackendProvider>,
  );
  expect(screen.getByText("Example Registry")).toBeTruthy();
  expect(
    screen.getByText("https://sources.example.com/registry.json"),
  ).toBeTruthy();
  expect(screen.getByText("https://broken.example.com")).toBeTruthy();
  expect(screen.getByRole("alert").textContent).toContain(
    "network: connection refused",
  );
});

test("adding a registry asks for confirmation and names the maintainer", async () => {
  const created: RegistryStatus = {
    ...registry,
    registry_id: "new",
    url: "https://new.example.com/registry.json",
  };
  const addSourceRegistry = vi.fn().mockResolvedValue(created);
  const listSourceMarket = vi.fn().mockResolvedValue([]);
  const backend = createMockBackend({
    overrides: { catalog: { addSourceRegistry, listSourceMarket } },
  });
  preset([]);
  render(
    <BackendProvider backend={backend}>
      <SourceRegistries />
    </BackendProvider>,
  );
  fireEvent.change(
    screen.getByRole("textbox", {
      name: "settings.streaming.registries.urlLabel",
    }),
    { target: { value: "https://new.example.com/registry.json" } },
  );
  fireEvent.click(
    screen.getByRole("button", {
      name: "settings.streaming.registries.add",
    }),
  );
  const dialog = screen.getByRole("dialog");
  expect(dialog.textContent).toContain("https://new.example.com/registry.json");
  expect(dialog.textContent).toContain("settings.streaming.registries.addBody");
  expect(addSourceRegistry).not.toHaveBeenCalled();
  fireEvent.click(
    screen.getByRole("button", {
      name: "settings.streaming.registries.addConfirm",
    }),
  );
  await waitFor(() =>
    expect(addSourceRegistry).toHaveBeenCalledWith(
      "https://new.example.com/registry.json",
    ),
  );
  await waitFor(() => expect(listSourceMarket).toHaveBeenCalled());
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(screen.getByText("Example Registry")).toBeTruthy();
});

test("a refused registry add surfaces the refusal message", async () => {
  const addSourceRegistry = vi.fn().mockRejectedValue({
    code: "streaming_source_unavailable",
    message:
      "invalid_registry: Another added URL already serves the registry id example.",
    retryable: false,
  });
  const backend = createMockBackend({
    overrides: { catalog: { addSourceRegistry } },
  });
  preset([]);
  render(
    <BackendProvider backend={backend}>
      <SourceRegistries />
    </BackendProvider>,
  );
  fireEvent.change(
    screen.getByRole("textbox", {
      name: "settings.streaming.registries.urlLabel",
    }),
    { target: { value: "https://copy.example.com/registry.json" } },
  );
  fireEvent.click(
    screen.getByRole("button", {
      name: "settings.streaming.registries.add",
    }),
  );
  fireEvent.click(
    screen.getByRole("button", {
      name: "settings.streaming.registries.addConfirm",
    }),
  );
  await waitFor(() => {
    expect(screen.getByRole("alert").textContent).toContain(
      "invalid_registry: Another added URL already serves the registry id example.",
    );
  });
  expect(screen.queryByRole("dialog")).toBeNull();
});

test("removing a registry confirms and explains that installs keep working", async () => {
  const removeSourceRegistry = vi.fn().mockResolvedValue(undefined);
  const listSourceMarket = vi.fn().mockResolvedValue([]);
  const backend = createMockBackend({
    overrides: { catalog: { removeSourceRegistry, listSourceMarket } },
  });
  preset([registry]);
  render(
    <BackendProvider backend={backend}>
      <SourceRegistries />
    </BackendProvider>,
  );
  fireEvent.click(
    screen.getByRole("button", {
      name: "settings.streaming.registries.remove",
    }),
  );
  const dialog = screen.getByRole("dialog");
  expect(dialog.textContent).toContain(
    "settings.streaming.registries.removeBody",
  );
  expect(dialog.textContent).toContain("Example Registry");
  expect(removeSourceRegistry).not.toHaveBeenCalled();
  fireEvent.click(
    screen.getByRole("button", {
      name: "settings.streaming.registries.removeConfirm",
    }),
  );
  await waitFor(() =>
    expect(removeSourceRegistry).toHaveBeenCalledWith("example"),
  );
  await waitFor(() => expect(listSourceMarket).toHaveBeenCalled());
  expect(screen.queryByText("Example Registry")).toBeNull();
});
