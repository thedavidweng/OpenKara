// @vitest-environment jsdom

import { act } from "react";
import { createRoot } from "react-dom/client";
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
  PlaybackStage: () => <div>Stage</div>,
}));

vi.mock("./FullscreenControls", () => ({
  FullscreenControls: () => <div>Controls</div>,
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

describe("FullscreenPlayerView romanization receiver mount order", () => {
  beforeEach(() => {
    mockAnnounceLocalAudienceOutputActive.mockReset();
    mockAnnounceLocalAudienceOutputActive.mockResolvedValue(undefined);
    mockUseLocalAudienceRomanizeReceiver.mockReset();
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
