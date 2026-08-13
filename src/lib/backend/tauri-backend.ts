import { createCdgCommands } from "@/lib/tauri/cdg";
import { tauriInvoke, type InvokeCommand } from "@/lib/tauri/invoke";
import { createLibraryCommands } from "@/lib/tauri/library";
import { createLibrarySetupCommands } from "@/lib/tauri/library-setup";
import { createLyricsCommands } from "@/lib/tauri/lyrics";
import { createMaintenanceCommands } from "@/lib/tauri/maintenance";
import { createPlaybackCommands } from "@/lib/tauri/playback";
import { createPlaylistCommands } from "@/lib/tauri/playlist";
import { createRemoteRepositoryCommands } from "@/lib/tauri/remote-repository";
import { createSeparationCommands } from "@/lib/tauri/separation";
import { createSettingsCommands } from "@/lib/tauri/settings";
import type { Backend } from "./types";

export function createTauriBackend(invoke: InvokeCommand): Backend {
  return {
    playback: createPlaybackCommands(invoke),
    library: createLibraryCommands(invoke),
    librarySetup: createLibrarySetupCommands(invoke),
    remoteRepository: createRemoteRepositoryCommands(invoke),
    settings: createSettingsCommands(invoke),
    lyrics: createLyricsCommands(invoke),
    separation: createSeparationCommands(invoke),
    maintenance: createMaintenanceCommands(invoke),
    playlist: createPlaylistCommands(invoke),
    cdg: createCdgCommands(invoke),
  };
}

export const tauriBackend: Backend = createTauriBackend(tauriInvoke);
