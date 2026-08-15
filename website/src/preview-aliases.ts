export const PREVIEW_ROMANIZER_MODULE_PATTERN = /^@\/lib\/lyrics-romanizer$/;
export const PREVIEW_I18N_MODULE_PATTERN = /^@\/lib\/i18n$/;

export const UNUSED_PREVIEW_MODULES = [
  "components/Settings/SettingsOverlay",
  "components/Settings/LibrarySetup",
  "components/Settings/ConfirmationDialog",
  "components/Settings/InputDialog",
  "components/Player/QueuePanel",
  "components/Lyrics/LyricsEditDialog",
  "components/Layout/UpdateBanner",
  "components/Layout/GlobalProgressBar",
  "components/Layout/ToastContainer",
  "components/Library/ImportCdgChoiceDialog",
  "components/Bootstrap/ModelBootstrapBanner",
  "components/Bootstrap/RuntimeUpdateBanner",
] as const;

export function escapeRegExpLiteral(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

export function previewUnusedModulePattern(modulePath: string): RegExp {
  return new RegExp(`^@/${escapeRegExpLiteral(modulePath)}$`);
}
