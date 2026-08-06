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
  useLocalAudienceRomanizeReceiver();

  useEffect(() => {
    void announceLocalAudienceOutputActive(true).catch(() => {});

    return () => {
      void announceLocalAudienceOutputActive(false).catch(() => {});
    };
  }, []);

  return (
    <div className="relative flex h-screen w-screen flex-col bg-black">
      <div className="flex flex-1 overflow-hidden">
        <PlaybackStage presentation="audience" />
      </div>
      <FullscreenControls />
    </div>
  );
}
