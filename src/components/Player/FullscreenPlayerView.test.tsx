// @vitest-environment jsdom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { beforeEach, describe, expect, test, vi } from "vitest";
import { FullscreenPlayerView } from "./FullscreenPlayerView";

const {
  mockAnnounceLocalAudienceOutputActive,
  mockUseLocalAudienceRomanizeReceiver,
} = vi.hoisted(() => ({
  mockAnnounceLocalAudienceOutputActive: vi.fn(),
  mockUseLocalAudienceRomanizeReceiver: vi.fn(),
}));

vi.mock("@/components/Playback/PlaybackStage", () => ({
  PlaybackStage: ({
    presentation,
    bottomInsetPx,
  }: {
    presentation?: string;
    bottomInsetPx?: number;
  }) => (
    <div
      data-testid="playback-stage"
      data-presentation={presentation}
      data-bottom-inset={bottomInsetPx}
    >
      Stage
    </div>
  ),
}));

vi.mock("./FullscreenControls", () => ({
  FullscreenControls: ({
    onHeightChange,
  }: {
    onHeightChange?: (height: number) => void;
  }) => (
    <div
      data-testid="fullscreen-controls"
      data-has-height-change={String(typeof onHeightChange === "function")}
    >
      Controls
    </div>
  ),
}));

vi.mock("@/hooks/use-cdg-frame-receiver", () => ({
  useCdgFrameReceiver: () => {},
}));

vi.mock("@/hooks/use-local-audience-romanize-receiver", () => ({
  useLocalAudienceRomanizeReceiver: mockUseLocalAudienceRomanizeReceiver,
}));

vi.mock("@/hooks/use-playback-runtime", () => ({
  useFullscreenPlaybackRuntime: () => {},
  useLyricsAutoFetch: () => {},
}));

vi.mock("@/lib/plain-text-page-controls", () => ({
  announceLocalAudienceOutputActive: mockAnnounceLocalAudienceOutputActive,
}));

describe("FullscreenPlayerView", () => {
  beforeEach(() => {
    mockAnnounceLocalAudienceOutputActive.mockReset();
    mockAnnounceLocalAudienceOutputActive.mockResolvedValue(undefined);
    mockUseLocalAudienceRomanizeReceiver.mockReset();
    mockUseLocalAudienceRomanizeReceiver.mockImplementation(() => {});
  });

  test("passes audience presentation and a persistent bottom inset to the stage", () => {
    const markup = renderToStaticMarkup(<FullscreenPlayerView />);

    expect(markup).toContain(
      "relative flex h-screen w-screen flex-col bg-black",
    );
    expect(markup).toContain("flex flex-1 overflow-hidden");
    expect(markup).toContain('data-presentation="audience"');
    expect(markup).toContain('data-bottom-inset="144"');
    expect(markup).toContain('data-has-height-change="true"');
    expect(markup).toContain("playback-stage");
  });

  test("announces when the local audience window opens and closes", async () => {
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    await act(async () => {
      root.render(<FullscreenPlayerView />);
    });

    expect(mockAnnounceLocalAudienceOutputActive).toHaveBeenCalledWith(true);

    await act(async () => {
      root.unmount();
    });

    expect(mockAnnounceLocalAudienceOutputActive).toHaveBeenLastCalledWith(
      false,
    );
    container.remove();
  });

  test("mounts the romanization receiver before announcing local audience output active", async () => {
    const callOrder: string[] = [];
    mockUseLocalAudienceRomanizeReceiver.mockImplementation(() => {
      callOrder.push("receiver-mounted");
    });
    mockAnnounceLocalAudienceOutputActive.mockImplementation(async () => {
      callOrder.push("announce-active");
    });

    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    await act(async () => {
      root.render(<FullscreenPlayerView />);
    });

    // The receiver hook runs synchronously during render, before the
    // announce effect's async call resolves.
    expect(callOrder.indexOf("receiver-mounted")).toBeLessThan(
      callOrder.indexOf("announce-active"),
    );

    await act(async () => {
      root.unmount();
    });
    container.remove();
  });
});
