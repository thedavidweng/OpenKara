import type { TFunction } from "i18next";
import type { RemoteLibraryProvider } from "@/types/ipc";

export function getRemoteProviderDisplayName(
  t: TFunction,
  provider: RemoteLibraryProvider,
): string {
  void provider;
  return t("settings.library.remoteLibraryDisplayName");
}

export function getRemoteProviderLabel(
  t: TFunction,
  provider: RemoteLibraryProvider,
): string {
  return provider === "google_drive"
    ? t("setup.remoteProvider.googleDrive.title")
    : provider === "dropbox"
      ? t("setup.remoteProvider.dropbox.title")
      : t("setup.remoteProvider.webdav.title");
}

export function getRemoteProviderConnectLabel(
  t: TFunction,
  provider: RemoteLibraryProvider,
): string {
  return provider === "google_drive"
    ? t("settings.library.connectRemoteGoogleDrive")
    : provider === "dropbox"
      ? t("settings.library.connectRemoteDropbox")
      : t("settings.library.connectRemoteWebdav");
}

export function getRemoteProviderBrowserSignInOpenedMessage(
  t: TFunction,
  provider: RemoteLibraryProvider,
): string | null {
  if (provider === "google_drive") {
    return t("settings.library.googleSignInOpened");
  }

  if (provider === "dropbox") {
    return t("settings.library.dropboxSignInOpened");
  }

  return null;
}

export function getRemoteProviderAuthTimeoutMessage(
  t: TFunction,
  provider: RemoteLibraryProvider,
): string {
  return provider === "google_drive"
    ? t("settings.library.googleSignInTimedOut")
    : provider === "dropbox"
      ? t("settings.library.dropboxSignInTimedOut")
      : t("settings.library.remoteSignInTimedOut");
}

export function getRemoteLibraryConnectedMessage(
  t: TFunction,
  provider: RemoteLibraryProvider,
): string {
  void provider;
  return t("settings.library.remoteLibraryConnected");
}
