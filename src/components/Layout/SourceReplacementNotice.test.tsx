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
import { useSettingsStore } from "@/stores/settings-store";
import type { RetiredSource } from "@/types/ipc";
import { SourceReplacementNotice } from "./SourceReplacementNotice";

vi.mock("react-i18next", () => ({
  initReactI18next: { type: "3rdParty", init: () => {} },
  useTranslation: () => ({
    t: (key: string, params?: Record<string, unknown>) =>
      params ? `${key} ${JSON.stringify(params)}` : key,
    i18n: { language: "en" },
  }),
}));

afterEach(cleanup);

const retired: RetiredSource[] = [
  { source_id: "netease", name: "NetEase Cloud Music", version: "1.4.0" },
  { source_id: "fixture", name: "Fixture Source", version: "1.0.0" },
];

function renderNotice(catalog: {
  getSourceReplacementNotice: () => Promise<RetiredSource[]>;
  dismissSourceReplacementNotice?: () => Promise<void>;
}) {
  render(
    <BackendProvider backend={createMockBackend({ overrides: { catalog } })}>
      <SourceReplacementNotice />
    </BackendProvider>,
  );
}

test("names every retired source and the steps to replace it", async () => {
  renderNotice({ getSourceReplacementNotice: async () => retired });
  const notice = await screen.findByRole("region", {
    name: "settings.streaming.replacementNotice.title",
  });
  expect(notice.textContent).toContain(
    '"names":"NetEase Cloud Music, Fixture Source"',
  );
});

test("dismissing hides the notice and records it once", async () => {
  const dismissSourceReplacementNotice = vi.fn().mockResolvedValue(undefined);
  renderNotice({
    getSourceReplacementNotice: async () => retired,
    dismissSourceReplacementNotice,
  });
  fireEvent.click(
    await screen.findByRole("button", {
      name: "settings.streaming.replacementNotice.dismiss",
    }),
  );
  await waitFor(() =>
    expect(dismissSourceReplacementNotice).toHaveBeenCalledTimes(1),
  );
  expect(screen.queryByRole("region")).toBeNull();
});

test("the notice opens Settings without dismissing itself", async () => {
  const dismissSourceReplacementNotice = vi.fn().mockResolvedValue(undefined);
  useSettingsStore.setState({ isOpen: false });
  renderNotice({
    getSourceReplacementNotice: async () => retired,
    dismissSourceReplacementNotice,
  });
  fireEvent.click(
    await screen.findByRole("button", {
      name: "settings.streaming.replacementNotice.openSettings",
    }),
  );
  expect(useSettingsStore.getState().isOpen).toBe(true);
  expect(dismissSourceReplacementNotice).not.toHaveBeenCalled();
  expect(screen.getByRole("region")).toBeTruthy();
});

test("nothing renders when no source was retired", async () => {
  const getSourceReplacementNotice = vi.fn().mockResolvedValue([]);
  renderNotice({ getSourceReplacementNotice });
  await waitFor(() => expect(getSourceReplacementNotice).toHaveBeenCalled());
  expect(screen.queryByRole("region")).toBeNull();
});
