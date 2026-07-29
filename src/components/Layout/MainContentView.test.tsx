// @vitest-environment jsdom

import { renderToStaticMarkup } from "react-dom/server";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { MainContentView } from "./MainContentView";

const { mockSettingsState, mockQueueState } = vi.hoisted(() => ({
  mockSettingsState: {
    isOpen: false,
  },
  mockQueueState: {
    isOpen: false,
  },
}));

vi.mock("@/stores/settings-store", () => ({
  useSettingsStore: (selector: (state: typeof mockSettingsState) => unknown) =>
    selector(mockSettingsState),
}));

vi.mock("@/stores/queue-store", () => ({
  useQueueStore: (selector: (state: typeof mockQueueState) => unknown) =>
    selector(mockQueueState),
}));

vi.mock("@/components/Layout/GlobalProgressBar", () => ({
  GlobalProgressBar: () => null,
}));

vi.mock("@/components/Playback/PlaybackStage", () => ({
  PlaybackStage: () => <div data-playback-stage="true" />,
}));

vi.mock("@/components/Player/PlaybackBar", () => ({
  PlaybackBar: () => <div data-playback-bar="true" />,
}));

vi.mock("@/components/Settings/SettingsOverlay", () => ({
  SettingsOverlay: () => <div data-settings-overlay="true" />,
}));

vi.mock("@/components/Bootstrap/ModelBootstrapBanner", () => ({
  ModelBootstrapBanner: () => null,
}));

vi.mock("@/components/Player/QueuePanel", () => ({
  QueuePanel: () => null,
}));

describe("MainContentView", () => {
  test("marks the main column with the unified shell variant", () => {
    const markup = renderToStaticMarkup(<MainContentView />);

    expect(markup).toContain('data-main-content-visual-variant="unified"');
    expect(markup).toContain('data-shell-content-pocket="true"');
    expect(markup).not.toContain("m-3");
    expect(markup).not.toContain("border-r");
    expect(markup).toContain('data-playback-stage="true"');
    expect(markup).toContain('data-playback-bar="true"');
  });

  test("overlays settings without unmounting playback content", () => {
    mockSettingsState.isOpen = true;

    const markup = renderToStaticMarkup(<MainContentView />);

    expect(markup).toContain('data-settings-overlay="true"');
    expect(markup).toContain('data-playback-stage="true"');
    expect(markup).not.toContain("data-native-floating-controls");

    mockSettingsState.isOpen = false;
  });

  test("keeps the real transport bar visible in the stable product preview", () => {
    const markup = renderToStaticMarkup(<MainContentView previewMode />);

    expect(markup).toContain('data-playback-stage="true"');
    expect(markup).toContain('data-playback-bar="true"');
  });
});

describe("MainContentView queue animation", () => {
  beforeEach(() => {
    (
      globalThis as typeof globalThis & {
        IS_REACT_ACT_ENVIRONMENT?: boolean;
      }
    ).IS_REACT_ACT_ENVIRONMENT = true;
  });

  afterEach(() => {
    document.body.innerHTML = "";
    mockQueueState.isOpen = false;
  });

  test("dispatches show when queue opens and exited when animation ends", () => {
    mockQueueState.isOpen = false;
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);

    act(() => {
      root.render(<MainContentView />);
    });

    mockQueueState.isOpen = true;
    act(() => {
      root.render(<MainContentView />);
    });

    mockQueueState.isOpen = false;
    act(() => {
      root.render(<MainContentView />);
    });

    const queueWrapper = host.querySelector(".animate-slide-out-right");
    expect(queueWrapper).toBeTruthy();
    act(() => {
      queueWrapper?.dispatchEvent(new Event("animationend", { bubbles: true }));
    });

    act(() => {
      root.unmount();
    });
  });
});
