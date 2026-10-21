// @vitest-environment jsdom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { useCatalogStore } from "@/stores/catalog-store";
import type { SourceSnapshot } from "@/types/ipc";
import { MainContentView } from "./MainContentView";

// Real stores on purpose: a conditional store hook only crashes when the real
// hook runs, so mocked selectors would never catch the regression.
vi.mock("@/components/Layout/GlobalProgressBar", () => ({
  GlobalProgressBar: () => null,
}));
vi.mock("@/components/Playback/PlaybackStage", () => ({
  PlaybackStage: () => <div data-playback-stage="true" />,
}));
vi.mock("@/components/Catalog/StreamingBrowser", () => ({
  StreamingBrowser: () => <div data-streaming-browser="true" />,
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
    useCatalogStore.setState({
      sources: [{ source_id: "alpha", enabled: true } as SourceSnapshot],
      importRun: null,
    });
    useCatalogStore.setState({ activeView: "library", activeSourceId: null });
  });

  afterEach(() => {
    document.body.innerHTML = "";
  });

  test("switching from library to a source does not change the hook order", () => {
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);

    act(() => {
      root.render(<MainContentView previewMode />);
    });
    expect(host.querySelector("[data-playback-stage]")).toBeTruthy();

    act(() => {
      useCatalogStore.setState({
        activeView: "streaming",
        activeSourceId: "alpha",
      });
    });
    expect(host.querySelector("[data-streaming-browser]")).toBeTruthy();
    expect(host.querySelector("[data-playback-stage]")).toBeNull();

    act(() => {
      useCatalogStore.setState({ activeView: "library", activeSourceId: null });
    });
    expect(host.querySelector("[data-playback-stage]")).toBeTruthy();

    act(() => {
      root.unmount();
    });
  });

  test("stays on the playback stage when the source is switched off", () => {
    useCatalogStore.setState({
      sources: [{ source_id: "alpha", enabled: false } as SourceSnapshot],
    });
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);
    act(() => {
      root.render(<MainContentView previewMode />);
    });
    act(() => {
      useCatalogStore.setState({
        activeView: "streaming",
        activeSourceId: "alpha",
      });
    });
    expect(host.querySelector("[data-playback-stage]")).toBeTruthy();
    act(() => {
      root.unmount();
    });
  });
});
