// @vitest-environment jsdom
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { BackendProvider } from "@/lib/backend";
import { createMockBackend } from "@/lib/backend/mock-backend";
import { useCatalogStore } from "@/stores/catalog-store";
import { StreamingSources } from "./StreamingSources";
vi.mock("react-i18next", () => ({
  initReactI18next: { type: "3rdParty", init: () => {} },
  useTranslation: () => ({ t: (key: string) => key }),
}));
afterEach(cleanup);
test("uninstall requires an explicit credential choice and active-work stop decision", async () => {
  const source = {
    source_id: "fixture",
    owner_binding: "test/fixture",
    name: "Fixture",
    description: "A source",
    license: "MIT",
    installed_version: "1.0.0",
    available_version: "1.0.0",
    staged_version: null,
    pinned_version: null,
    enabled: true,
    runtime_ready: true,
    running: true,
    state: "ready" as const,
    progress: null,
    last_error: null,
    active_work: [{ id: 1, kind: "preview" as const, cancelling: false }],
    manifest: null,
  };
  const uninstall = vi.fn().mockResolvedValue({
    ...source,
    installed_version: null,
    enabled: false,
    active_work: [],
  });
  const backend = createMockBackend({
    overrides: {
      catalog: {
        listStreamingSources: async () => [source],
        uninstallStreamingSource: uninstall,
      },
    },
  });
  useCatalogStore.setState({
    sources: [source],
    sourceStates: {},
    sourcesStatus: "ready",
    sourcesError: null,
  });
  render(
    <BackendProvider backend={backend}>
      <StreamingSources />
    </BackendProvider>,
  );
  fireEvent.click(
    screen.getByRole("button", { name: "settings.streaming.uninstall" }),
  );
  expect(screen.getByRole("dialog").textContent).toContain(
    "settings.streaming.stopWorkBody",
  );
  expect(uninstall).not.toHaveBeenCalled();
  expect(
    (
      screen.getByRole("button", {
        name: "settings.streaming.confirmUninstall",
      }) as HTMLButtonElement
    ).disabled,
  ).toBe(true);
  fireEvent.click(
    screen.getByRole("radio", { name: "settings.streaming.retainCredentials" }),
  );
  fireEvent.click(
    screen.getByRole("checkbox", { name: "settings.streaming.stopWork" }),
  );
  fireEvent.click(
    screen.getByRole("button", { name: "settings.streaming.confirmUninstall" }),
  );
  await waitFor(() =>
    expect(uninstall).toHaveBeenCalledWith("fixture", true, false),
  );
});

test("declining active-work interruption leaves the source enabled", () => {
  const source = {
    source_id: "fixture",
    owner_binding: "test/fixture",
    name: "Fixture",
    description: "A source",
    license: "MIT",
    installed_version: "1.0.0",
    available_version: "1.0.0",
    staged_version: null,
    pinned_version: null,
    enabled: true,
    runtime_ready: true,
    running: true,
    state: "ready" as const,
    progress: null,
    last_error: null,
    active_work: [{ id: 1, kind: "import" as const, cancelling: false }],
    manifest: null,
  };
  const disable = vi.fn();
  const backend = createMockBackend({
    overrides: { catalog: { setStreamingSourceEnabled: disable } },
  });
  useCatalogStore.setState({
    sources: [source],
    sourceStates: {},
    sourcesStatus: "ready",
    sourcesError: null,
  });
  render(
    <BackendProvider backend={backend}>
      <StreamingSources />
    </BackendProvider>,
  );
  fireEvent.click(
    screen.getByRole("button", { name: "settings.streaming.disable" }),
  );
  expect(screen.getByRole("dialog").textContent).toContain(
    "settings.streaming.work.import",
  );
  fireEvent.click(screen.getByRole("button", { name: "common.cancel" }));
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(
    screen.getByRole("button", { name: "settings.streaming.disable" }),
  ).toBeTruthy();
  expect(disable).not.toHaveBeenCalled();
});
