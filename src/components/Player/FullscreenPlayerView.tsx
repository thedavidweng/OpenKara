import { useEffect } from "react";
import { PlaybackStage } from "@/components/Playback/PlaybackStage";
import { useCdgFrameReceiver } from "@/hooks/use-cdg-frame-receiver";
import { useLocalAudienceRomanizeReceiver } from "@/hooks/use-local-audience-romanize-receiver";
import {
  useFullscreenPlaybackRuntime,
  useLyricsAutoFetch,
} from "@/hooks/use-playback-runtime";
import { announceLocalAudienceOutputActive } from "@/lib/plain-text-page-controls";
import { FullscreenControls } from "./FullscreenControls";

export function FullscreenPlayerView() {
  useFullscreenPlaybackRuntime();
  useLyricsAutoFetch();
  useCdgFrameReceiver();
  // Mount the romanization receiver before the audience-active announcement
  // so its state listener is registered before the main window emits the
  // initial authoritative snapshot in response to the sync request.
  useLocalAudienceRomanizeReceiver();

  useEffect(() => {
    void announceLocalAudienceOutputActive(true).catch(() => {
      // The main window treats this as auxiliary state; a missed update must
      // not block opening the audience window itself.
    });

    return () => {
      void announceLocalAudienceOutputActive(false).catch(() => {
        // Closing the window should stay best-effort even if the state sync is gone.
      });
    };
  }, []);

  // RATIONALE: the stage spans the full window height. Reserving a permanent
  // band for the auto-hiding controls left a dead black strip along the
  // bottom of the audience screen even while the controls were hidden; the
  // controls float above the lyrics with their own scrim instead.
  return (
    <div className="relative flex h-screen w-screen flex-col bg-black">
      <div className="flex flex-1 overflow-hidden">
        <PlaybackStage presentation="audience" />
      </div>
      <FullscreenControls />
    </div>
  );
}
