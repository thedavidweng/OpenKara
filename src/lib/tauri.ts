import { tauriBackend } from "@/lib/backend";
import { tauriInvoke } from "@/lib/tauri/invoke";
import type { CoverArtBytes, CoverArtSize } from "@/types/ipc";

export type { RemoteConflictResolution } from "@/lib/backend";

export type * from "./tauri/cdg";
export type * from "./tauri/library-setup";
export type * from "./tauri/library";
export type * from "./tauri/lyrics";
export type * from "./tauri/maintenance";
export type * from "./tauri/playback";
export type * from "./tauri/playlist";
export type * from "./tauri/remote-repository";
export type * from "./tauri/separation";
export type * from "./tauri/settings";

export function addSongsToPlaylist(
  ...args: Parameters<typeof tauriBackend.playlist.addSongsToPlaylist>
): ReturnType<typeof tauriBackend.playlist.addSongsToPlaylist> {
  return tauriBackend.playlist.addSongsToPlaylist(...args);
}
export function advanceRotation(
  ...args: Parameters<typeof tauriBackend.playlist.advanceRotation>
): ReturnType<typeof tauriBackend.playlist.advanceRotation> {
  return tauriBackend.playlist.advanceRotation(...args);
}
export function batchSeparate(
  ...args: Parameters<typeof tauriBackend.maintenance.batchSeparate>
): ReturnType<typeof tauriBackend.maintenance.batchSeparate> {
  return tauriBackend.maintenance.batchSeparate(...args);
}
export function beginRemoteAuth(
  ...args: Parameters<typeof tauriBackend.remoteRepository.beginRemoteAuth>
): ReturnType<typeof tauriBackend.remoteRepository.beginRemoteAuth> {
  return tauriBackend.remoteRepository.beginRemoteAuth(...args);
}
export function cancelBatchSeparation(
  ...args: Parameters<typeof tauriBackend.maintenance.cancelBatchSeparation>
): ReturnType<typeof tauriBackend.maintenance.cancelBatchSeparation> {
  return tauriBackend.maintenance.cancelBatchSeparation(...args);
}
export function cancelRemoteAuth(
  ...args: Parameters<typeof tauriBackend.remoteRepository.cancelRemoteAuth>
): ReturnType<typeof tauriBackend.remoteRepository.cancelRemoteAuth> {
  return tauriBackend.remoteRepository.cancelRemoteAuth(...args);
}
export function cancelSeparation(
  ...args: Parameters<typeof tauriBackend.separation.cancelSeparation>
): ReturnType<typeof tauriBackend.separation.cancelSeparation> {
  return tauriBackend.separation.cancelSeparation(...args);
}
export function checkLibraryIntegrity(
  ...args: Parameters<typeof tauriBackend.library.checkLibraryIntegrity>
): ReturnType<typeof tauriBackend.library.checkLibraryIntegrity> {
  return tauriBackend.library.checkLibraryIntegrity(...args);
}
export function checkModelUpdates(
  ...args: Parameters<typeof tauriBackend.settings.checkModelUpdates>
): ReturnType<typeof tauriBackend.settings.checkModelUpdates> {
  return tauriBackend.settings.checkModelUpdates(...args);
}
export function checkRuntimeUpdates(
  ...args: Parameters<typeof tauriBackend.settings.checkRuntimeUpdates>
): ReturnType<typeof tauriBackend.settings.checkRuntimeUpdates> {
  return tauriBackend.settings.checkRuntimeUpdates(...args);
}
export function clearRemoteCache(
  ...args: Parameters<typeof tauriBackend.remoteRepository.clearRemoteCache>
): ReturnType<typeof tauriBackend.remoteRepository.clearRemoteCache> {
  return tauriBackend.remoteRepository.clearRemoteCache(...args);
}
export function createLocalLibrary(
  ...args: Parameters<typeof tauriBackend.librarySetup.createLocalLibrary>
): ReturnType<typeof tauriBackend.librarySetup.createLocalLibrary> {
  return tauriBackend.librarySetup.createLocalLibrary(...args);
}
export function createPlaylist(
  ...args: Parameters<typeof tauriBackend.playlist.createPlaylist>
): ReturnType<typeof tauriBackend.playlist.createPlaylist> {
  return tauriBackend.playlist.createPlaylist(...args);
}
export function createRemoteLibrary(
  ...args: Parameters<typeof tauriBackend.remoteRepository.createRemoteLibrary>
): ReturnType<typeof tauriBackend.remoteRepository.createRemoteLibrary> {
  return tauriBackend.remoteRepository.createRemoteLibrary(...args);
}
export function deleteAllCachedLyrics(
  ...args: Parameters<typeof tauriBackend.maintenance.deleteAllCachedLyrics>
): ReturnType<typeof tauriBackend.maintenance.deleteAllCachedLyrics> {
  return tauriBackend.maintenance.deleteAllCachedLyrics(...args);
}
export function deleteAllStems(
  ...args: Parameters<typeof tauriBackend.maintenance.deleteAllStems>
): ReturnType<typeof tauriBackend.maintenance.deleteAllStems> {
  return tauriBackend.maintenance.deleteAllStems(...args);
}
export function deleteLibrary(
  ...args: Parameters<typeof tauriBackend.librarySetup.deleteLibrary>
): ReturnType<typeof tauriBackend.librarySetup.deleteLibrary> {
  return tauriBackend.librarySetup.deleteLibrary(...args);
}
export function deleteModel(
  ...args: Parameters<typeof tauriBackend.settings.deleteModel>
): ReturnType<typeof tauriBackend.settings.deleteModel> {
  return tauriBackend.settings.deleteModel(...args);
}
export function deletePlaylist(
  ...args: Parameters<typeof tauriBackend.playlist.deletePlaylist>
): ReturnType<typeof tauriBackend.playlist.deletePlaylist> {
  return tauriBackend.playlist.deletePlaylist(...args);
}
export function deleteRuntime(
  ...args: Parameters<typeof tauriBackend.settings.deleteRuntime>
): ReturnType<typeof tauriBackend.settings.deleteRuntime> {
  return tauriBackend.settings.deleteRuntime(...args);
}
export function deleteSongs(
  ...args: Parameters<typeof tauriBackend.library.deleteSongs>
): ReturnType<typeof tauriBackend.library.deleteSongs> {
  return tauriBackend.library.deleteSongs(...args);
}
export function downgradeAllToTwoStem(
  ...args: Parameters<typeof tauriBackend.maintenance.downgradeAllToTwoStem>
): ReturnType<typeof tauriBackend.maintenance.downgradeAllToTwoStem> {
  return tauriBackend.maintenance.downgradeAllToTwoStem(...args);
}
export function downgradeToTwoStem(
  ...args: Parameters<typeof tauriBackend.maintenance.downgradeToTwoStem>
): ReturnType<typeof tauriBackend.maintenance.downgradeToTwoStem> {
  return tauriBackend.maintenance.downgradeToTwoStem(...args);
}
export function downloadModel(
  ...args: Parameters<typeof tauriBackend.settings.downloadModel>
): ReturnType<typeof tauriBackend.settings.downloadModel> {
  return tauriBackend.settings.downloadModel(...args);
}
export function downloadRuntime(
  ...args: Parameters<typeof tauriBackend.settings.downloadRuntime>
): ReturnType<typeof tauriBackend.settings.downloadRuntime> {
  return tauriBackend.settings.downloadRuntime(...args);
}
export function estimateDowngradeSavings(
  ...args: Parameters<typeof tauriBackend.maintenance.estimateDowngradeSavings>
): ReturnType<typeof tauriBackend.maintenance.estimateDowngradeSavings> {
  return tauriBackend.maintenance.estimateDowngradeSavings(...args);
}
export function estimateStemsSize(
  ...args: Parameters<typeof tauriBackend.maintenance.estimateStemsSize>
): ReturnType<typeof tauriBackend.maintenance.estimateStemsSize> {
  return tauriBackend.maintenance.estimateStemsSize(...args);
}
export function expandImportPaths(
  ...args: Parameters<typeof tauriBackend.library.expandImportPaths>
): ReturnType<typeof tauriBackend.library.expandImportPaths> {
  return tauriBackend.library.expandImportPaths(...args);
}
export function extractEmbeddedCoverArt(
  ...args: Parameters<typeof tauriBackend.maintenance.extractEmbeddedCoverArt>
): ReturnType<typeof tauriBackend.maintenance.extractEmbeddedCoverArt> {
  return tauriBackend.maintenance.extractEmbeddedCoverArt(...args);
}
export function extractEmbeddedLyrics(
  ...args: Parameters<typeof tauriBackend.lyrics.extractEmbeddedLyrics>
): ReturnType<typeof tauriBackend.lyrics.extractEmbeddedLyrics> {
  return tauriBackend.lyrics.extractEmbeddedLyrics(...args);
}
export function fetchLyrics(
  ...args: Parameters<typeof tauriBackend.lyrics.fetchLyrics>
): ReturnType<typeof tauriBackend.lyrics.fetchLyrics> {
  return tauriBackend.lyrics.fetchLyrics(...args);
}
export function fetchLyricsOnline(
  ...args: Parameters<typeof tauriBackend.lyrics.fetchLyricsOnline>
): ReturnType<typeof tauriBackend.lyrics.fetchLyricsOnline> {
  return tauriBackend.lyrics.fetchLyricsOnline(...args);
}
export function getActiveLibrary(
  ...args: Parameters<typeof tauriBackend.librarySetup.getActiveLibrary>
): ReturnType<typeof tauriBackend.librarySetup.getActiveLibrary> {
  return tauriBackend.librarySetup.getActiveLibrary(...args);
}
export function getAllSeparationStatuses(
  ...args: Parameters<typeof tauriBackend.separation.getAllSeparationStatuses>
): ReturnType<typeof tauriBackend.separation.getAllSeparationStatuses> {
  return tauriBackend.separation.getAllSeparationStatuses(...args);
}
export function getAllUploadStatuses(
  ...args: Parameters<typeof tauriBackend.remoteRepository.getAllUploadStatuses>
): ReturnType<typeof tauriBackend.remoteRepository.getAllUploadStatuses> {
  return tauriBackend.remoteRepository.getAllUploadStatuses(...args);
}
export function getAudioPeaks(
  ...args: Parameters<typeof tauriBackend.playback.getAudioPeaks>
): ReturnType<typeof tauriBackend.playback.getAudioPeaks> {
  return tauriBackend.playback.getAudioPeaks(...args);
}
export function getCdgFrame(
  ...args: Parameters<typeof tauriBackend.cdg.getCdgFrame>
): ReturnType<typeof tauriBackend.cdg.getCdgFrame> {
  return tauriBackend.cdg.getCdgFrame(...args);
}
export function getCdgStatus(
  ...args: Parameters<typeof tauriBackend.cdg.getCdgStatus>
): ReturnType<typeof tauriBackend.cdg.getCdgStatus> {
  return tauriBackend.cdg.getCdgStatus(...args);
}
export function getCoverArtPreview(
  ...args: Parameters<typeof tauriBackend.library.getCoverArtPreview>
): ReturnType<typeof tauriBackend.library.getCoverArtPreview> {
  return tauriBackend.library.getCoverArtPreview(...args);
}
export function getCoverArtThumbnail(
  ...args: Parameters<typeof tauriBackend.library.getCoverArtThumbnail>
): ReturnType<typeof tauriBackend.library.getCoverArtThumbnail> {
  return tauriBackend.library.getCoverArtThumbnail(...args);
}
export function getDebugInfo(
  ...args: Parameters<typeof tauriBackend.settings.getDebugInfo>
): ReturnType<typeof tauriBackend.settings.getDebugInfo> {
  return tauriBackend.settings.getDebugInfo(...args);
}
export function getImportCandidateDetails(
  ...args: Parameters<typeof tauriBackend.library.getImportCandidateDetails>
): ReturnType<typeof tauriBackend.library.getImportCandidateDetails> {
  return tauriBackend.library.getImportCandidateDetails(...args);
}
export function getLibrary(
  ...args: Parameters<typeof tauriBackend.library.getLibrary>
): ReturnType<typeof tauriBackend.library.getLibrary> {
  return tauriBackend.library.getLibrary(...args);
}
export function getLibraryPath(
  ...args: Parameters<typeof tauriBackend.librarySetup.getLibraryPath>
): ReturnType<typeof tauriBackend.librarySetup.getLibraryPath> {
  return tauriBackend.librarySetup.getLibraryPath(...args);
}
export function getLibraryRegistry(
  ...args: Parameters<typeof tauriBackend.librarySetup.getLibraryRegistry>
): ReturnType<typeof tauriBackend.librarySetup.getLibraryRegistry> {
  return tauriBackend.librarySetup.getLibraryRegistry(...args);
}
export function getModelBootstrapStatus(
  ...args: Parameters<typeof tauriBackend.settings.getModelBootstrapStatus>
): ReturnType<typeof tauriBackend.settings.getModelBootstrapStatus> {
  return tauriBackend.settings.getModelBootstrapStatus(...args);
}
export function getModelStatus(
  ...args: Parameters<typeof tauriBackend.settings.getModelStatus>
): ReturnType<typeof tauriBackend.settings.getModelStatus> {
  return tauriBackend.settings.getModelStatus(...args);
}
export function getPlaybackState(
  ...args: Parameters<typeof tauriBackend.playback.getPlaybackState>
): ReturnType<typeof tauriBackend.playback.getPlaybackState> {
  return tauriBackend.playback.getPlaybackState(...args);
}
export function getPlaylistSongs(
  ...args: Parameters<typeof tauriBackend.playlist.getPlaylistSongs>
): ReturnType<typeof tauriBackend.playlist.getPlaylistSongs> {
  return tauriBackend.playlist.getPlaylistSongs(...args);
}
export function getRemoteCacheUsage(
  ...args: Parameters<typeof tauriBackend.remoteRepository.getRemoteCacheUsage>
): ReturnType<typeof tauriBackend.remoteRepository.getRemoteCacheUsage> {
  return tauriBackend.remoteRepository.getRemoteCacheUsage(...args);
}
export function getRemoteDiagnostics(
  ...args: Parameters<typeof tauriBackend.remoteRepository.getRemoteDiagnostics>
): ReturnType<typeof tauriBackend.remoteRepository.getRemoteDiagnostics> {
  return tauriBackend.remoteRepository.getRemoteDiagnostics(...args);
}
export function getRotationState(
  ...args: Parameters<typeof tauriBackend.playlist.getRotationState>
): ReturnType<typeof tauriBackend.playlist.getRotationState> {
  return tauriBackend.playlist.getRotationState(...args);
}
export function getRuntimeBootstrapStatus(
  ...args: Parameters<typeof tauriBackend.settings.getRuntimeBootstrapStatus>
): ReturnType<typeof tauriBackend.settings.getRuntimeBootstrapStatus> {
  return tauriBackend.settings.getRuntimeBootstrapStatus(...args);
}
export function getSeparationStatus(
  ...args: Parameters<typeof tauriBackend.separation.getSeparationStatus>
): ReturnType<typeof tauriBackend.separation.getSeparationStatus> {
  return tauriBackend.separation.getSeparationStatus(...args);
}
export function getSettings(
  ...args: Parameters<typeof tauriBackend.settings.getSettings>
): ReturnType<typeof tauriBackend.settings.getSettings> {
  return tauriBackend.settings.getSettings(...args);
}
export function getSongProperties(
  ...args: Parameters<typeof tauriBackend.library.getSongProperties>
): ReturnType<typeof tauriBackend.library.getSongProperties> {
  return tauriBackend.library.getSongProperties(...args);
}
export function getWaveform(
  ...args: Parameters<typeof tauriBackend.playback.getWaveform>
): ReturnType<typeof tauriBackend.playback.getWaveform> {
  return tauriBackend.playback.getWaveform(...args);
}
export function getWindowShellState(
  ...args: Parameters<typeof tauriBackend.settings.getWindowShellState>
): ReturnType<typeof tauriBackend.settings.getWindowShellState> {
  return tauriBackend.settings.getWindowShellState(...args);
}
export function importLyricsFiles(
  ...args: Parameters<typeof tauriBackend.lyrics.importLyricsFiles>
): ReturnType<typeof tauriBackend.lyrics.importLyricsFiles> {
  return tauriBackend.lyrics.importLyricsFiles(...args);
}
export function importSongs(
  ...args: Parameters<typeof tauriBackend.library.importSongs>
): ReturnType<typeof tauriBackend.library.importSongs> {
  return tauriBackend.library.importSongs(...args);
}
export function listPlaylists(
  ...args: Parameters<typeof tauriBackend.playlist.listPlaylists>
): ReturnType<typeof tauriBackend.playlist.listPlaylists> {
  return tauriBackend.playlist.listPlaylists(...args);
}
export function listRemoteLibraryRoots(
  ...args: Parameters<typeof tauriBackend.remoteRepository.listRemoteLibraryRoots>
): ReturnType<typeof tauriBackend.remoteRepository.listRemoteLibraryRoots> {
  return tauriBackend.remoteRepository.listRemoteLibraryRoots(...args);
}
export function loadStems(
  ...args: Parameters<typeof tauriBackend.playback.loadStems>
): ReturnType<typeof tauriBackend.playback.loadStems> {
  return tauriBackend.playback.loadStems(...args);
}
export function mirrorLocalLibraryToRemote(
  ...args: Parameters<typeof tauriBackend.remoteRepository.mirrorLocalLibraryToRemote>
): ReturnType<typeof tauriBackend.remoteRepository.mirrorLocalLibraryToRemote> {
  return tauriBackend.remoteRepository.mirrorLocalLibraryToRemote(...args);
}
export function openExternalUrl(
  ...args: Parameters<typeof tauriBackend.remoteRepository.openExternalUrl>
): ReturnType<typeof tauriBackend.remoteRepository.openExternalUrl> {
  return tauriBackend.remoteRepository.openExternalUrl(...args);
}
export function pause(
  ...args: Parameters<typeof tauriBackend.playback.pause>
): ReturnType<typeof tauriBackend.playback.pause> {
  return tauriBackend.playback.pause(...args);
}
export function pickImportPaths(
  ...args: Parameters<typeof tauriBackend.library.pickImportPaths>
): ReturnType<typeof tauriBackend.library.pickImportPaths> {
  return tauriBackend.library.pickImportPaths(...args);
}
export function play(
  ...args: Parameters<typeof tauriBackend.playback.play>
): ReturnType<typeof tauriBackend.playback.play> {
  return tauriBackend.playback.play(...args);
}
export function pollRemoteAuth(
  ...args: Parameters<typeof tauriBackend.remoteRepository.pollRemoteAuth>
): ReturnType<typeof tauriBackend.remoteRepository.pollRemoteAuth> {
  return tauriBackend.remoteRepository.pollRemoteAuth(...args);
}
export function publishSongToRemote(
  ...args: Parameters<typeof tauriBackend.remoteRepository.publishSongToRemote>
): ReturnType<typeof tauriBackend.remoteRepository.publishSongToRemote> {
  return tauriBackend.remoteRepository.publishSongToRemote(...args);
}
export function publishSongsToRemote(
  ...args: Parameters<typeof tauriBackend.remoteRepository.publishSongsToRemote>
): ReturnType<typeof tauriBackend.remoteRepository.publishSongsToRemote> {
  return tauriBackend.remoteRepository.publishSongsToRemote(...args);
}
export function reSeparate(
  ...args: Parameters<typeof tauriBackend.separation.reSeparate>
): ReturnType<typeof tauriBackend.separation.reSeparate> {
  return tauriBackend.separation.reSeparate(...args);
}
export function reauthorizeRemoteRepository(
  ...args: Parameters<typeof tauriBackend.remoteRepository.reauthorizeRemoteRepository>
): ReturnType<typeof tauriBackend.remoteRepository.reauthorizeRemoteRepository> {
  return tauriBackend.remoteRepository.reauthorizeRemoteRepository(...args);
}
export function refreshRemoteRepository(
  ...args: Parameters<typeof tauriBackend.remoteRepository.refreshRemoteRepository>
): ReturnType<typeof tauriBackend.remoteRepository.refreshRemoteRepository> {
  return tauriBackend.remoteRepository.refreshRemoteRepository(...args);
}
export function registerLocalLibrary(
  ...args: Parameters<typeof tauriBackend.librarySetup.registerLocalLibrary>
): ReturnType<typeof tauriBackend.librarySetup.registerLocalLibrary> {
  return tauriBackend.librarySetup.registerLocalLibrary(...args);
}
export function registerRemoteLibrary(
  ...args: Parameters<typeof tauriBackend.remoteRepository.registerRemoteLibrary>
): ReturnType<typeof tauriBackend.remoteRepository.registerRemoteLibrary> {
  return tauriBackend.remoteRepository.registerRemoteLibrary(...args);
}
export function relocateRemoteRepository(
  ...args: Parameters<typeof tauriBackend.remoteRepository.relocateRemoteRepository>
): ReturnType<typeof tauriBackend.remoteRepository.relocateRemoteRepository> {
  return tauriBackend.remoteRepository.relocateRemoteRepository(...args);
}
export function removeLibrary(
  ...args: Parameters<typeof tauriBackend.librarySetup.removeLibrary>
): ReturnType<typeof tauriBackend.librarySetup.removeLibrary> {
  return tauriBackend.librarySetup.removeLibrary(...args);
}
export function removeMissingLibraryEntries(
  ...args: Parameters<typeof tauriBackend.library.removeMissingLibraryEntries>
): ReturnType<typeof tauriBackend.library.removeMissingLibraryEntries> {
  return tauriBackend.library.removeMissingLibraryEntries(...args);
}
export function removeSongsFromPlaylist(
  ...args: Parameters<typeof tauriBackend.playlist.removeSongsFromPlaylist>
): ReturnType<typeof tauriBackend.playlist.removeSongsFromPlaylist> {
  return tauriBackend.playlist.removeSongsFromPlaylist(...args);
}
export function renameLibrary(
  ...args: Parameters<typeof tauriBackend.librarySetup.renameLibrary>
): ReturnType<typeof tauriBackend.librarySetup.renameLibrary> {
  return tauriBackend.librarySetup.renameLibrary(...args);
}
export function renamePlaylist(
  ...args: Parameters<typeof tauriBackend.playlist.renamePlaylist>
): ReturnType<typeof tauriBackend.playlist.renamePlaylist> {
  return tauriBackend.playlist.renamePlaylist(...args);
}
export function resolveRemoteConflict(
  ...args: Parameters<typeof tauriBackend.remoteRepository.resolveRemoteConflict>
): ReturnType<typeof tauriBackend.remoteRepository.resolveRemoteConflict> {
  return tauriBackend.remoteRepository.resolveRemoteConflict(...args);
}
export function resolveRemoteLibraryCandidate(
  ...args: Parameters<typeof tauriBackend.remoteRepository.resolveRemoteLibraryCandidate>
): ReturnType<typeof tauriBackend.remoteRepository.resolveRemoteLibraryCandidate> {
  return tauriBackend.remoteRepository.resolveRemoteLibraryCandidate(...args);
}
export function restartApp(
  ...args: Parameters<typeof tauriBackend.settings.restartApp>
): ReturnType<typeof tauriBackend.settings.restartApp> {
  return tauriBackend.settings.restartApp(...args);
}
export function resume(
  ...args: Parameters<typeof tauriBackend.playback.resume>
): ReturnType<typeof tauriBackend.playback.resume> {
  return tauriBackend.playback.resume(...args);
}
export function saveManualLyrics(
  ...args: Parameters<typeof tauriBackend.lyrics.saveManualLyrics>
): ReturnType<typeof tauriBackend.lyrics.saveManualLyrics> {
  return tauriBackend.lyrics.saveManualLyrics(...args);
}
export function searchLibrary(
  ...args: Parameters<typeof tauriBackend.library.searchLibrary>
): ReturnType<typeof tauriBackend.library.searchLibrary> {
  return tauriBackend.library.searchLibrary(...args);
}
export function seek(
  ...args: Parameters<typeof tauriBackend.playback.seek>
): ReturnType<typeof tauriBackend.playback.seek> {
  return tauriBackend.playback.seek(...args);
}
export function separate(
  ...args: Parameters<typeof tauriBackend.separation.separate>
): ReturnType<typeof tauriBackend.separation.separate> {
  return tauriBackend.separation.separate(...args);
}
export function setCoverArtBackdrop(
  ...args: Parameters<typeof tauriBackend.settings.setCoverArtBackdrop>
): ReturnType<typeof tauriBackend.settings.setCoverArtBackdrop> {
  return tauriBackend.settings.setCoverArtBackdrop(...args);
}
export function setCrossfadeDurationMs(
  ...args: Parameters<typeof tauriBackend.settings.setCrossfadeDurationMs>
): ReturnType<typeof tauriBackend.settings.setCrossfadeDurationMs> {
  return tauriBackend.settings.setCrossfadeDurationMs(...args);
}
export function setCrossfadeEnabled(
  ...args: Parameters<typeof tauriBackend.settings.setCrossfadeEnabled>
): ReturnType<typeof tauriBackend.settings.setCrossfadeEnabled> {
  return tauriBackend.settings.setCrossfadeEnabled(...args);
}
export function setEqEnabled(
  ...args: Parameters<typeof tauriBackend.settings.setEqEnabled>
): ReturnType<typeof tauriBackend.settings.setEqEnabled> {
  return tauriBackend.settings.setEqEnabled(...args);
}
export function setEqGains(
  ...args: Parameters<typeof tauriBackend.settings.setEqGains>
): ReturnType<typeof tauriBackend.settings.setEqGains> {
  return tauriBackend.settings.setEqGains(...args);
}
export function setExecutionProvider(
  ...args: Parameters<typeof tauriBackend.settings.setExecutionProvider>
): ReturnType<typeof tauriBackend.settings.setExecutionProvider> {
  return tauriBackend.settings.setExecutionProvider(...args);
}
export function setHideBatchSeparate(
  ...args: Parameters<typeof tauriBackend.settings.setHideBatchSeparate>
): ReturnType<typeof tauriBackend.settings.setHideBatchSeparate> {
  return tauriBackend.settings.setHideBatchSeparate(...args);
}
export function setHideUpgradeAll(
  ...args: Parameters<typeof tauriBackend.settings.setHideUpgradeAll>
): ReturnType<typeof tauriBackend.settings.setHideUpgradeAll> {
  return tauriBackend.settings.setHideUpgradeAll(...args);
}
export function setLanguage(
  ...args: Parameters<typeof tauriBackend.settings.setLanguage>
): ReturnType<typeof tauriBackend.settings.setLanguage> {
  return tauriBackend.settings.setLanguage(...args);
}
export function setLibrarySortMode(
  ...args: Parameters<typeof tauriBackend.settings.setLibrarySortMode>
): ReturnType<typeof tauriBackend.settings.setLibrarySortMode> {
  return tauriBackend.settings.setLibrarySortMode(...args);
}
export function setLyricsFontStep(
  ...args: Parameters<typeof tauriBackend.settings.setLyricsFontStep>
): ReturnType<typeof tauriBackend.settings.setLyricsFontStep> {
  return tauriBackend.settings.setLyricsFontStep(...args);
}
export function setLyricsOffset(
  ...args: Parameters<typeof tauriBackend.lyrics.setLyricsOffset>
): ReturnType<typeof tauriBackend.lyrics.setLyricsOffset> {
  return tauriBackend.lyrics.setLyricsOffset(...args);
}
export function setModelVariant(
  ...args: Parameters<typeof tauriBackend.settings.setModelVariant>
): ReturnType<typeof tauriBackend.settings.setModelVariant> {
  return tauriBackend.settings.setModelVariant(...args);
}
export function setNativeAppMenuLabels(
  ...args: Parameters<typeof tauriBackend.settings.setNativeAppMenuLabels>
): ReturnType<typeof tauriBackend.settings.setNativeAppMenuLabels> {
  return tauriBackend.settings.setNativeAppMenuLabels(...args);
}
export function setNativeSidebarVisibility(
  ...args: Parameters<typeof tauriBackend.settings.setNativeSidebarVisibility>
): ReturnType<typeof tauriBackend.settings.setNativeSidebarVisibility> {
  return tauriBackend.settings.setNativeSidebarVisibility(...args);
}
export function setPreloadCandidate(
  ...args: Parameters<typeof tauriBackend.playback.setPreloadCandidate>
): ReturnType<typeof tauriBackend.playback.setPreloadCandidate> {
  return tauriBackend.playback.setPreloadCandidate(...args);
}
export function setQueueEntrySinger(
  ...args: Parameters<typeof tauriBackend.playlist.setQueueEntrySinger>
): ReturnType<typeof tauriBackend.playlist.setQueueEntrySinger> {
  return tauriBackend.playlist.setQueueEntrySinger(...args);
}
export function setRotationState(
  ...args: Parameters<typeof tauriBackend.playlist.setRotationState>
): ReturnType<typeof tauriBackend.playlist.setRotationState> {
  return tauriBackend.playlist.setRotationState(...args);
}
export function setSongsInstrumental(
  ...args: Parameters<typeof tauriBackend.library.setSongsInstrumental>
): ReturnType<typeof tauriBackend.library.setSongsInstrumental> {
  return tauriBackend.library.setSongsInstrumental(...args);
}
export function setSongsLanguage(
  ...args: Parameters<typeof tauriBackend.library.setSongsLanguage>
): ReturnType<typeof tauriBackend.library.setSongsLanguage> {
  return tauriBackend.library.setSongsLanguage(...args);
}
export function setStemMode(
  ...args: Parameters<typeof tauriBackend.settings.setStemMode>
): ReturnType<typeof tauriBackend.settings.setStemMode> {
  return tauriBackend.settings.setStemMode(...args);
}
export function setStemVolume(
  ...args: Parameters<typeof tauriBackend.playback.setStemVolume>
): ReturnType<typeof tauriBackend.playback.setStemVolume> {
  return tauriBackend.playback.setStemVolume(...args);
}
export function setThemePreference(
  ...args: Parameters<typeof tauriBackend.settings.setThemePreference>
): ReturnType<typeof tauriBackend.settings.setThemePreference> {
  return tauriBackend.settings.setThemePreference(...args);
}
export function setUpdatePolicy(
  ...args: Parameters<typeof tauriBackend.settings.setUpdatePolicy>
): ReturnType<typeof tauriBackend.settings.setUpdatePolicy> {
  return tauriBackend.settings.setUpdatePolicy(...args);
}
export function setVolume(
  ...args: Parameters<typeof tauriBackend.playback.setVolume>
): ReturnType<typeof tauriBackend.playback.setVolume> {
  return tauriBackend.playback.setVolume(...args);
}
export function stepAirPlayPlainTextPage(
  ...args: Parameters<typeof tauriBackend.playback.stepAirPlayPlainTextPage>
): ReturnType<typeof tauriBackend.playback.stepAirPlayPlainTextPage> {
  return tauriBackend.playback.stepAirPlayPlainTextPage(...args);
}
export function switchLibrary(
  ...args: Parameters<typeof tauriBackend.librarySetup.switchLibrary>
): ReturnType<typeof tauriBackend.librarySetup.switchLibrary> {
  return tauriBackend.librarySetup.switchLibrary(...args);
}
export function syncAirPlayAudienceState(
  ...args: Parameters<typeof tauriBackend.playback.syncAirPlayAudienceState>
): ReturnType<typeof tauriBackend.playback.syncAirPlayAudienceState> {
  return tauriBackend.playback.syncAirPlayAudienceState(...args);
}
export function syncAirPlayRoutePicker(
  ...args: Parameters<typeof tauriBackend.playback.syncAirPlayRoutePicker>
): ReturnType<typeof tauriBackend.playback.syncAirPlayRoutePicker> {
  return tauriBackend.playback.syncAirPlayRoutePicker(...args);
}
export function updateSongMetadata(
  ...args: Parameters<typeof tauriBackend.library.updateSongMetadata>
): ReturnType<typeof tauriBackend.library.updateSongMetadata> {
  return tauriBackend.library.updateSongMetadata(...args);
}
export function upgradeToFourStem(
  ...args: Parameters<typeof tauriBackend.separation.upgradeToFourStem>
): ReturnType<typeof tauriBackend.separation.upgradeToFourStem> {
  return tauriBackend.separation.upgradeToFourStem(...args);
}
export function windowReady(
  ...args: Parameters<typeof tauriBackend.settings.windowReady>
): ReturnType<typeof tauriBackend.settings.windowReady> {
  return tauriBackend.settings.windowReady(...args);
}

export function createLibrary(path: string): Promise<void> {
  return tauriBackend.librarySetup.createLocalLibrary(path);
}

export function openLibrary(path: string): Promise<void> {
  return tauriBackend.librarySetup.registerLocalLibrary(path);
}

export function getCoverArt(
  hash: string,
  size?: CoverArtSize,
): Promise<CoverArtBytes> {
  return tauriInvoke<CoverArtBytes>(
    "get_cover_art",
    size === undefined ? { hash } : { hash, size },
  );
}
