import type { YoutubeWatchAction, YoutubeWatchMediaState } from "@/types/ipc";
import type { InvokeCommand } from "./invoke";

export function createYoutubeWatchCommands(invoke: InvokeCommand) {
  return {
    controlYoutubeWatch: (action: YoutubeWatchAction) =>
      invoke<YoutubeWatchMediaState>("control_youtube_watch", { action }),
  };
}
