// @vitest-environment jsdom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { useCatalogStore } from "@/stores/catalog-store";
import { useSettingsStore } from "@/stores/settings-store";
import { MainContentView } from "./MainContentView";

// Real stores on purpose: a conditional store hook only crashes when the real
// hook runs, so mocked selectors would never catch the regression.
vi.mock("@/components/Layout/GlobalProgressBar", () => ({
  GlobalProgressBar: () => null,
}));
vi.mock("@/components/Playback/PlaybackStage", () => ({
  PlaybackStage: () => <div data-playback-stage="true" />,
}));
vi.mock("@/components/Catalog/NeteaseBrowser", () => ({
  NeteaseBrowser: () => <div data-netease-browser="true" />,
}));
vi.mock("@/components/Player/PlaybackBar", () => ({
  PlaybackBar: () => null,
}));
vi.mock("@/components/Settings/SettingsOverlay", () => ({
  SettingsOverlay: () => null,
}));
vi.mock("@/components/Bootstrap/ModelBootstrapBanner", () => ({
  ModelBootstrapBanner: () => null,
}));
vi.mock("@/components/Bootstrap/RuntimeUpdateBanner", () => ({
  RuntimeUpdateBanner: () => null,
}));
vi.mock("@/components/Layout/UpdateBanner", () => ({
  UpdateBanner: () => null,
}));
vi.mock("@/components/Player/QueuePanel", () => ({
  QueuePanel: () => null,
}));

describe("MainContentView view switching", () => {
  beforeEach(() => {
    (
      globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }
    ).IS_REACT_ACT_ENVIRONMENT = true;
    useSettingsStore.setState({ neteaseSourceEnabled: true });
    useCatalogStore.setState({ activeView: "library" });
  });

  afterEach(() => {
    document.body.innerHTML = "";
  });

  test("switching from library to netease does not change the hook order", () => {
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);

    act(() => {
      root.render(<MainContentView previewMode />);
    });
    expect(host.querySelector("[data-playback-stage]")).toBeTruthy();

    act(() => {
      useCatalogStore.setState({ activeView: "netease" });
    });
    expect(host.querySelector("[data-netease-browser]")).toBeTruthy();
    expect(host.querySelector("[data-playback-stage]")).toBeNull();

    act(() => {
      useCatalogStore.setState({ activeView: "library" });
    });
    expect(host.querySelector("[data-playback-stage]")).toBeTruthy();

    act(() => {
      root.unmount();
    });
  });

  test("stays on the playback stage when NetEase is switched off", () => {
    useSettingsStore.setState({ neteaseSourceEnabled: false });
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);
    act(() => {
      root.render(<MainContentView previewMode />);
    });
    act(() => {
      useCatalogStore.setState({ activeView: "netease" });
    });
    expect(host.querySelector("[data-playback-stage]")).toBeTruthy();
    act(() => {
      root.unmount();
    });
  });
});
