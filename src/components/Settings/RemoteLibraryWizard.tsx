import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Cloud, X } from "lucide-react";
import { getErrorMessage } from "@/lib/errors";
import { useModalDialog } from "@/hooks/use-modal-dialog";
import { useBackend } from "@/lib/backend";
import type { RegisteredLibrary, RemoteLibraryProvider } from "@/types/ipc";
import { useSettings } from "./SettingsController.context";
import {
  REMOTE_AUTH_CANCELLED,
  runRemoteLibraryRegistrationFlow,
} from "./remote-library-flow";
import {
  getRemoteProviderConnectLabel,
  getRemoteProviderDisplayName,
  getRemoteProviderLabel,
} from "./remote-library-copy";

type RemoteSetupMode = "open_remote" | "mirror_active_local";
type RemoteLibraryWizardPurpose = "add" | "reauthorize";

interface RemoteLibraryWizardProps {
  onClose: () => void;
  libraryId?: string;
  initialProvider?: RemoteLibraryProvider;
  initialDisplayName?: string;
  initialServerUrl?: string;
  initialRemoteRootLocator?: string;
  initialRemotePathDisplay?: string;
  initialRootPath?: string;
  purpose?: RemoteLibraryWizardPurpose;
}

export function RemoteLibraryWizard({
  onClose,
  libraryId,
  initialProvider = "google_drive",
  initialDisplayName,
  initialServerUrl = "",
  initialRootPath = "/OpenKara",
  initialRemoteRootLocator = initialRootPath,
  initialRemotePathDisplay,
  purpose = "add",
}: RemoteLibraryWizardProps) {
  const { remoteRepository } = useBackend();
  const { t } = useTranslation();
  const { view, library } = useSettings();
  const [mode, setMode] = useState<RemoteSetupMode>("open_remote");
  const [provider, setProvider] =
    useState<RemoteLibraryProvider>(initialProvider);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [authorizationUrl, setAuthorizationUrl] = useState<string | null>(null);
  const [displayName, setDisplayName] = useState(
    initialDisplayName ?? getRemoteProviderDisplayName(t, initialProvider),
  );
  const [serverUrl, setServerUrl] = useState(initialServerUrl);
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [rootPath, setRootPath] = useState(initialRootPath);
  const dialogRef = useRef<HTMLDivElement>(null);
  const closeButtonRef = useRef<HTMLButtonElement>(null);
  const titleId = "remote-library-wizard-title";
  const displayNameInputId = "remote-library-display-name";
  const serverUrlInputId = "remote-library-webdav-server-url";
  const rootPathInputId = "remote-library-webdav-root-path";
  const usernameInputId = "remote-library-webdav-username";
  const passwordInputId = "remote-library-webdav-password";

  const activeLibrary = view.library.libraries.find(
    (candidate) => candidate.id === view.library.activeLibraryId,
  );
  const activeLocalLibrary =
    activeLibrary?.kind === "local" ? activeLibrary : null;
  const canMirrorActiveLocal = activeLocalLibrary !== null;
  const isReauthorizeFlow = purpose === "reauthorize";
  const isRecoveryFlow = isReauthorizeFlow;
  const cancelledRef = useRef(false);
  const mountedRef = useRef(true);
  const authSessionIdRef = useRef<string | null>(null);

  useEffect(() => {
    return () => {
      cancelledRef.current = true;
      mountedRef.current = false;
      if (authSessionIdRef.current) {
        void remoteRepository.cancelRemoteAuth(authSessionIdRef.current);
      }
    };
  }, [remoteRepository]);

  const resetProviderState = (nextProvider: RemoteLibraryProvider) => {
    setProvider(nextProvider);
    setDisplayName(getRemoteProviderDisplayName(t, nextProvider));
    setError(null);
    setMessage(null);
    setAuthorizationUrl(null);
    authSessionIdRef.current = null;
  };

  const requestClose = () => {
    cancelledRef.current = true;
    if (authSessionIdRef.current) {
      void remoteRepository.cancelRemoteAuth(authSessionIdRef.current);
    }
    if (mountedRef.current) {
      setLoading(false);
    }
    onClose();
  };

  useModalDialog({
    dialogRef,
    initialFocusRef: closeButtonRef,
    onDismiss: requestClose,
  });

  const connect = async () => {
    cancelledRef.current = false;
    if (
      !isRecoveryFlow &&
      mode === "mirror_active_local" &&
      !activeLocalLibrary
    ) {
      setError(t("settings.library.mirrorActiveLocalDescriptionNoLocal"));
      return;
    }

    setLoading(true);
    setError(null);
    setMessage(null);
    setAuthorizationUrl(null);

    try {
      const runRegistration = async (relocate: boolean) =>
        runRemoteLibraryRegistrationFlow({
          provider,
          displayName,
          t,
          remoteApi: remoteRepository,
          libraryId: isReauthorizeFlow ? libraryId : undefined,
          existingRemoteRootLocator: isReauthorizeFlow
            ? initialRemoteRootLocator
            : undefined,
          existingRemotePathDisplay: isReauthorizeFlow
            ? initialRemotePathDisplay
            : undefined,
          relocate,
          webdav: {
            serverUrl,
            username,
            password,
            rootPath,
          },
          isCancelled: () => cancelledRef.current,
          onSessionIdChange: (sessionId) => {
            authSessionIdRef.current = sessionId;
          },
          onAuthorizationUrlChange: (nextAuthorizationUrl) => {
            if (mountedRef.current) {
              setAuthorizationUrl(nextAuthorizationUrl);
            }
          },
          onMessageChange: (nextMessage) => {
            if (mountedRef.current) {
              setMessage(nextMessage);
            }
          },
        });

      const result = await runRegistration(false);
      const { candidate } = result;
      let { registry } = result;

      if (
        isReauthorizeFlow &&
        candidate.remote_root_locator !== initialRemoteRootLocator
      ) {
        const confirmed = window.confirm(
          t("settings.library.confirmRemoteRepositoryRelocation", {
            nextLocation: candidate.remote_path_display,
          }),
        );
        if (!confirmed) {
          return;
        }
        const relocationResult = await runRegistration(true);
        registry = relocationResult.registry;
      }

      if (cancelledRef.current) {
        return;
      }

      const remoteLibraryId = isReauthorizeFlow
        ? libraryId
        : registry.active_library_id;

      if (!remoteLibraryId) {
        throw new Error(t("settings.library.remoteLibraryMissingId"));
      }

      const mirrorSource =
        !isRecoveryFlow && mode === "mirror_active_local"
          ? activeLocalLibrary
          : null;

      if (mirrorSource) {
        await remoteRepository.mirrorLocalLibraryToRemote(
          mirrorSource.id,
          remoteLibraryId,
        );
      }

      const activation = await library.activate(remoteLibraryId);
      if (!activation.ok) {
        setError(activation.error);
        return;
      }

      setMessage(
        mirrorSource
          ? t("settings.library.remoteLibraryCreatedAndMirroring", {
              displayName: mirrorSource.display_name,
            })
          : t("settings.library.remoteLibraryConnected"),
      );
      onClose();
    } catch (err: unknown) {
      if (getErrorMessage(err) !== REMOTE_AUTH_CANCELLED) {
        setError(getErrorMessage(err));
      }
    } finally {
      if (mountedRef.current && !cancelledRef.current) {
        setLoading(false);
      }
    }
  };

  const activateExistingLibrary = async (candidateId: string) => {
    setError(null);
    const activation = await library.activate(candidateId);
    if (!activation.ok) {
      setError(activation.error);
      return;
    }
    onClose();
  };

  const remoteLibraries = view.library.libraries.filter(
    (candidate): candidate is Extract<RegisteredLibrary, { kind: "remote" }> =>
      candidate.kind === "remote",
  );

  const titleKey = isReauthorizeFlow
    ? "settings.library.reauthorizeRemoteRepository"
    : "settings.library.addRemoteLibrary";
  const descriptionKey = isReauthorizeFlow
    ? "settings.library.reauthorizeRemoteRepositoryDescription"
    : "settings.library.addRemoteLibraryDescription";

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4">
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-busy={loading}
        tabIndex={-1}
        className="w-full max-w-xl rounded-xl border border-[var(--color-border-light)] bg-[var(--color-sidebar)] p-5 shadow-2xl"
      >
        <div className="mb-4 flex items-start justify-between gap-4">
          <div>
            <h2
              id={titleId}
              className="text-lg font-semibold text-[var(--color-text)]"
            >
              {t(titleKey)}
            </h2>
            <p className="mt-1 text-sm text-[var(--color-text-dim)]">
              {t(descriptionKey)}
            </p>
          </div>
          <button
            ref={closeButtonRef}
            type="button"
            onClick={requestClose}
            aria-label={t("common.close")}
            className="rounded-md p-1 text-[var(--color-text-dim)] transition-colors hover:bg-[var(--color-hover)] hover:text-[var(--color-text)]"
          >
            <X size={16} />
          </button>
        </div>

        {!isRecoveryFlow && (
          <div className="grid gap-3 md:grid-cols-2">
            <button
              type="button"
              onClick={() => setMode("open_remote")}
              disabled={loading}
              aria-pressed={mode === "open_remote"}
              className={`rounded-lg border px-4 py-3 text-left ${
                mode === "open_remote"
                  ? "border-[var(--color-control-selected-border)] bg-[var(--color-control-selected-bg)]"
                  : "border-[var(--color-border-light)] bg-[var(--color-surface)]"
              }`}
            >
              <p className="text-sm font-medium text-[var(--color-text)]">
                {t("settings.library.openRemoteLibrary")}
              </p>
              <p className="mt-1 text-xs text-[var(--color-text-dim)]">
                {t("settings.library.openRemoteLibraryDescription")}
              </p>
            </button>
            <button
              type="button"
              onClick={() => setMode("mirror_active_local")}
              disabled={loading || !canMirrorActiveLocal}
              aria-pressed={mode === "mirror_active_local"}
              className={`rounded-lg border px-4 py-3 text-left ${
                mode === "mirror_active_local"
                  ? "border-[var(--color-control-selected-border)] bg-[var(--color-control-selected-bg)]"
                  : "border-[var(--color-border-light)] bg-[var(--color-surface)]"
              } disabled:opacity-50`}
            >
              <p className="text-sm font-medium text-[var(--color-text)]">
                {t("settings.library.createAndMirrorActiveLocal")}
              </p>
              <p className="mt-1 text-xs text-[var(--color-text-dim)]">
                {activeLocalLibrary
                  ? t("settings.library.mirrorActiveLocalDescriptionWithName", {
                      displayName: activeLocalLibrary.display_name,
                    })
                  : t("settings.library.mirrorActiveLocalDescriptionNoLocal")}
              </p>
            </button>
          </div>
        )}

        <div className="mt-4 grid gap-3 md:grid-cols-3">
          {(
            [
              ["google_drive", Cloud],
              ["dropbox", Cloud],
              ["webdav", Cloud],
            ] as const
          ).map(([candidate, Icon]) => (
            <button
              key={candidate}
              type="button"
              onClick={() => resetProviderState(candidate)}
              disabled={loading}
              aria-pressed={provider === candidate}
              className={`rounded-lg border px-4 py-3 text-left ${
                provider === candidate
                  ? "border-[var(--color-control-selected-border)] bg-[var(--color-control-selected-bg)]"
                  : "border-[var(--color-border-light)] bg-[var(--color-surface)]"
              }`}
            >
              <Icon
                size={16}
                className="mb-2 text-[var(--color-control-primary)]"
              />
              <p className="text-sm font-medium text-[var(--color-text)]">
                {getRemoteProviderLabel(t, candidate)}
              </p>
            </button>
          ))}
        </div>

        <div className="mt-4 space-y-3 rounded-lg border border-[var(--color-border-light)] bg-[var(--color-surface)] p-4">
          <div>
            <label
              htmlFor={displayNameInputId}
              className="mb-1 block text-xs font-medium text-[var(--color-text)]"
            >
              {t("settings.library.displayName")}
            </label>
            <input
              id={displayNameInputId}
              value={displayName}
              onChange={(event) => setDisplayName(event.target.value)}
              className="w-full rounded-md border border-[var(--color-border-light)] bg-[var(--color-sidebar)] px-3 py-2 text-sm text-[var(--color-text)] outline-none focus:border-[var(--color-accent)]"
            />
          </div>

          {provider === "google_drive" && (
            <p className="text-xs text-[var(--color-text-dim)]">
              {t("settings.library.googleDriveBundledDescription")}
            </p>
          )}

          {provider === "dropbox" && (
            <p className="text-xs text-[var(--color-text-dim)]">
              {t("settings.library.dropboxBundledDescription")}
            </p>
          )}

          {provider === "webdav" && (
            <>
              <div>
                <label
                  htmlFor={serverUrlInputId}
                  className="mb-1 block text-xs font-medium text-[var(--color-text)]"
                >
                  {t("settings.library.webdavServerUrl")}
                </label>
                <input
                  id={serverUrlInputId}
                  type="url"
                  value={serverUrl}
                  onChange={(event) => setServerUrl(event.target.value)}
                  className="w-full rounded-md border border-[var(--color-border-light)] bg-[var(--color-sidebar)] px-3 py-2 text-sm text-[var(--color-text)] outline-none focus:border-[var(--color-accent)]"
                  spellCheck={false}
                />
              </div>
              <div>
                <label
                  htmlFor={rootPathInputId}
                  className="mb-1 block text-xs font-medium text-[var(--color-text)]"
                >
                  {t("settings.library.webdavLibraryPath")}
                </label>
                <input
                  id={rootPathInputId}
                  value={rootPath}
                  onChange={(event) => setRootPath(event.target.value)}
                  className="w-full rounded-md border border-[var(--color-border-light)] bg-[var(--color-sidebar)] px-3 py-2 text-sm text-[var(--color-text)] outline-none focus:border-[var(--color-accent)]"
                  spellCheck={false}
                />
              </div>
              <div className="grid gap-3 md:grid-cols-2">
                <div>
                  <label
                    htmlFor={usernameInputId}
                    className="mb-1 block text-xs font-medium text-[var(--color-text)]"
                  >
                    {t("settings.library.webdavUsername")}
                  </label>
                  <input
                    id={usernameInputId}
                    value={username}
                    onChange={(event) => setUsername(event.target.value)}
                    className="w-full rounded-md border border-[var(--color-border-light)] bg-[var(--color-sidebar)] px-3 py-2 text-sm text-[var(--color-text)] outline-none focus:border-[var(--color-accent)]"
                    spellCheck={false}
                    autoComplete="username"
                  />
                </div>
                <div>
                  <label
                    htmlFor={passwordInputId}
                    className="mb-1 block text-xs font-medium text-[var(--color-text)]"
                  >
                    {t("settings.library.webdavPassword")}
                  </label>
                  <input
                    id={passwordInputId}
                    type="password"
                    value={password}
                    onChange={(event) => setPassword(event.target.value)}
                    autoComplete="current-password"
                    className="w-full rounded-md border border-[var(--color-border-light)] bg-[var(--color-sidebar)] px-3 py-2 text-sm text-[var(--color-text)] outline-none focus:border-[var(--color-accent)]"
                  />
                </div>
              </div>
            </>
          )}

          <button
            type="button"
            onClick={() => void connect()}
            disabled={loading || view.isInitializing}
            className="w-full rounded-lg bg-[var(--color-control-primary)] px-4 py-2.5 text-sm font-medium text-[var(--color-control-primary-foreground)] transition-opacity hover:opacity-90 disabled:opacity-60"
          >
            {loading
              ? t("settings.library.connecting")
              : !isRecoveryFlow && mode === "mirror_active_local"
                ? t("settings.library.createRemoteLibraryAndStartMirror")
                : isReauthorizeFlow
                  ? t("settings.library.reauthorizeRemoteRepository")
                  : getRemoteProviderConnectLabel(t, provider)}
          </button>

          {authorizationUrl && (
            <a
              href={authorizationUrl}
              target="_blank"
              rel="noreferrer"
              className="block text-xs text-[var(--color-accent)] underline underline-offset-2"
            >
              {t("settings.library.openBrowserSignInAgain")}
            </a>
          )}

          {error && (
            <p role="alert" className="text-sm text-[var(--color-destructive)]">
              {error}
            </p>
          )}
          {message && (
            <p
              role="status"
              aria-live="polite"
              aria-atomic="true"
              className="text-sm text-[var(--color-text-dim)]"
            >
              {message}
            </p>
          )}
        </div>

        {remoteLibraries.length > 0 && (
          <div className="mt-4 rounded-lg border border-[var(--color-border-light)] bg-[var(--color-surface)] p-4">
            <p className="mb-2 text-xs font-medium text-[var(--color-text-dim)]">
              {t("settings.library.existingRemoteLibraries")}
            </p>
            <div className="space-y-2">
              {remoteLibraries.map((candidate) => (
                <button
                  key={candidate.id}
                  type="button"
                  onClick={() => void activateExistingLibrary(candidate.id)}
                  disabled={loading}
                  className="rounded-md border border-[var(--color-border-light)] px-3 py-2"
                >
                  <p className="text-sm text-[var(--color-text)]">
                    {candidate.display_name}
                  </p>
                  <p className="text-xs text-[var(--color-text-dim)]">
                    {candidate.remote_path_display ||
                      candidate.remote_root_locator}
                  </p>
                </button>
              ))}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
