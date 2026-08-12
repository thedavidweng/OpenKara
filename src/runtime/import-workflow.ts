import type { TFunction } from "i18next";
import type { LibraryBackend, LyricsBackend } from "@/lib/backend";
import i18next from "@/lib/i18n";
import { notifySuccess } from "@/lib/errors";
import type { ImportFailure, LyricsMatch, Song } from "@/types/ipc";
import {
  buildAmbiguousCdgChoiceRequests,
  buildImportSongsOptions,
  type AmbiguousCdgChoiceRequest,
  type ExplicitCdgSelection,
} from "@/lib/import-cdg-selection";

export type ImportWorkflowApi = Pick<
  LibraryBackend,
  "importSongs" | "getLibrary"
> &
  Pick<LyricsBackend, "importLyricsFiles">;

export interface RunImportWorkflowOptions {
  paths: string[];
  api: ImportWorkflowApi;
  promptForCdgChoice: (
    request: AmbiguousCdgChoiceRequest,
  ) => Promise<string | null>;
  notifyError: (error: unknown) => void;
  setImportErrors: (errors: ImportFailure[]) => void;
  setSongs: (songs: Song[]) => void;
  publishLibraryInvalidation: () => void;
  t?: TFunction;
}

function songDisplayName(match: LyricsMatch): string {
  const title = match.song_title ?? "";
  const artist = match.song_artist ?? "";
  if (title && artist) {
    return `${title} — ${artist}`;
  }
  return title || artist || match.song_id.slice(0, 8);
}

export async function runImportWorkflow({
  paths,
  api,
  promptForCdgChoice,
  notifyError,
  setImportErrors,
  setSongs,
  publishLibraryInvalidation,
}: RunImportWorkflowOptions) {
  const audioPaths = paths.filter((p) => !p.toLowerCase().endsWith(".lrc"));
  const lrcPaths = paths.filter((p) => p.toLowerCase().endsWith(".lrc"));
  const explicitSelections: ExplicitCdgSelection[] = [];
  const excludedAmbiguousAudioPaths = new Set<string>();

  for (const request of buildAmbiguousCdgChoiceRequests(audioPaths)) {
    const selectedAudioPath = await promptForCdgChoice(request);
    if (selectedAudioPath) {
      for (const candidate of request.audioCandidates) {
        if (candidate !== selectedAudioPath) {
          excludedAmbiguousAudioPaths.add(candidate);
        }
      }
      explicitSelections.push({
        audioPath: selectedAudioPath,
        cdgPath: request.cdgPath,
      });
    }
  }

  const audioPathsToImport = audioPaths.filter(
    (path) => !excludedAmbiguousAudioPaths.has(path),
  );

  if (audioPathsToImport.length > 0) {
    const result = await api.importSongs(
      audioPathsToImport,
      buildImportSongsOptions(explicitSelections),
    );
    if (result.failed.length > 0) {
      setImportErrors(result.failed);
      for (const failure of result.failed) {
        notifyError(failure.error);
      }
    }
  }

  if (lrcPaths.length > 0) {
    const lrcResult = await api.importLyricsFiles(lrcPaths);
    for (const match of lrcResult.matched) {
      const fileName = match.lrc_path.split(/[/\\]/).pop() ?? match.lrc_path;
      notifySuccess(
        i18next.t("lyrics.matchedToast", {
          song: songDisplayName(match),
        }),
        fileName,
      );
    }
    for (const path of lrcResult.unmatched) {
      notifyError(i18next.t("lyrics.unmatchedToast", { file: path }));
    }
  }

  const songs = await api.getLibrary();
  setSongs(songs);
  publishLibraryInvalidation();
}
