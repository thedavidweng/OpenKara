import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { open } from "@tauri-apps/plugin-dialog";
import {
  type LucideIcon,
  Cloud,
  FolderOpen,
  Plus,
  Music,
  Globe,
  Layers,
  Mic2,
  ChevronLeft,
  Check,
  Search,
} from "lucide-react";
import { getErrorMessage } from "@/lib/errors";
import * as api from "@/lib/tauri";
import i18next, { SUPPORTED_LANGUAGES, detectSystemLanguage } from "@/lib/i18n";
import { useSettingsStore } from "@/stores/settings-store";
import type { RemoteLibraryProvider } from "@/types/ipc";
import { runRemoteLibraryRegistrationFlow } from "./remote-library-flow";
import {
  getRemoteLibraryConnectedMessage,
  getRemoteProviderDisplayName,
} from "./remote-library-copy";

type Step = "language" | "library" | "remoteProvider" | "stemMode";
type LibraryChoiceKind = "create_local" | "open_local" | "open_remote";
type SetupTranslationKey =
  | "setup.createNew"
  | "setup.createNewDescription"
  | "setup.openExisting"
  | "setup.openExistingDescription"
  | "setup.useRemoteRepository"
  | "setup.openRemoteLibraryDescription"
  | "setup.remoteProvider.googleDrive.title"
  | "setup.remoteProvider.googleDrive.description"
  | "setup.remoteProvider.dropbox.title"
  | "setup.remoteProvider.dropbox.description"
  | "setup.remoteProvider.webdav.title"
  | "setup.remoteProvider.webdav.description";

interface RemoteProviderChoice {
  provider: RemoteLibraryProvider;
  icon: LucideIcon;
  title: SetupTranslationKey;
  description: SetupTranslationKey;
  availableNow: boolean;
}

interface LibraryChoice {
  kind: LibraryChoiceKind;
  icon: LucideIcon;
  title: SetupTranslationKey;
  description: SetupTranslationKey;
}

// oxlint-disable-next-line react/only-export-components
export const librarySetupChoices: LibraryChoice[] = [
  {
    kind: "create_local",
    icon: Plus,
    title: "setup.createNew",
    description: "setup.createNewDescription",
  },
  {
    kind: "open_local",
    icon: FolderOpen,
    title: "setup.openExisting",
    description: "setup.openExistingDescription",
  },
  {
    kind: "open_remote",
    icon: Globe,
    title: "setup.useRemoteRepository",
    description: "setup.openRemoteLibraryDescription",
  },
];

// oxlint-disable-next-line react/only-export-components
export const remoteLibraryProviders: RemoteProviderChoice[] = [
  {
    provider: "google_drive",
    icon: Cloud,
    title: "setup.remoteProvider.googleDrive.title",
    description: "setup.remoteProvider.googleDrive.description",
    availableNow: true,
  },
  {
    provider: "dropbox",
    icon: Cloud,
    title: "setup.remoteProvider.dropbox.title",
    description: "setup.remoteProvider.dropbox.description",
    availableNow: true,
  },
  {
    provider: "webdav",
    icon: Cloud,
    title: "setup.remoteProvider.webdav.title",
    description: "setup.remoteProvider.webdav.description",
    availableNow: true,
  },
];

interface LibrarySetupProps {
  onComplete: () => void;
}

const WEBDAV_SERVER_URL_ID = "library-setup-webdav-server-url";
const WEBDAV_LIBRARY_PATH_ID = "library-setup-webdav-library-path";
const WEBDAV_USERNAME_ID = "library-setup-webdav-username";
const WEBDAV_PASSWORD_ID = "library-setup-webdav-password";

function StepIndicator({ current }: { current: Step }) {
  const steps: Step[] = ["language", "library", "stemMode"];
  const currentIndex = steps.indexOf(
    current === "remoteProvider" ? "library" : current,
  );

  return (
    <div className="flex items-center justify-center gap-2">
      {steps.map((step, i) => (
        <div
          key={step}
          className={`h-1.5 w-1.5 rounded-full transition-colors ${
            i <= currentIndex
              ? "bg-[var(--color-control-primary)]"
              : "bg-[var(--color-border)]"
          }`}
        />
      ))}
    </div>
  );
}

function languageBadge(code: string): string {
  const overrides: Record<string, string> = {
    en: "EN",
    "zh-CN": "简",
    "zh-TW": "繁",
  };
  return overrides[code] ?? code.split("-")[0].slice(0, 2).toUpperCase();
}

export function LibrarySetup({ onComplete }: LibrarySetupProps) {
  const { t } = useTranslation();
  const settingsLanguage = useSettingsStore((s) => s.language);
  const settingsStemMode = useSettingsStore((s) => s.stemMode);
  const patchAppSettings = useSettingsStore((s) => s.patchAppSettings);
  const hydrateAppSettings = useSettingsStore((s) => s.hydrateAppSettings);
  const [step, setStep] = useState<Step>("language");
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [selectedRemoteProvider, setSelectedRemoteProvider] =
    useState<RemoteLibraryProvider | null>(null);
  const [remoteMessage, setRemoteMessage] = useState<string | null>(null);
  const [remoteAuthorizationUrl, setRemoteAuthorizationUrl] = useState<
    string | null
  >(null);
  const [remoteDisplayName, setRemoteDisplayName] = useState(() =>
    getRemoteProviderDisplayName(t, "google_drive"),
  );
  const [remoteServerUrl, setRemoteServerUrl] = useState("");
  const [remoteUsername, setRemoteUsername] = useState("");
  const [remotePassword, setRemotePassword] = useState("");
  const [remoteRootPath, setRemoteRootPath] = useState("/OpenKara");
  const [selectedLanguageDraft, setSelectedLanguageDraft] = useState<
    string | null
  >(null);
  const [languageFilter, setLanguageFilter] = useState("");
  const [selectedStemModeDraft, setSelectedStemModeDraft] = useState<
    "two_stem" | "four_stem" | null
  >(null);
  const remoteAuthSessionIdRef = useRef<string | null>(null);
  const selectedLanguage =
    selectedLanguageDraft ??
    settingsLanguage ??
    i18next.resolvedLanguage ??
    detectSystemLanguage();
  const selectedStemMode = selectedStemModeDraft ?? settingsStemMode;

  const resolveSingleDirectory = (selected: string | string[] | null) =>
    typeof selected === "string" ? selected : (selected?.[0] ?? null);

  const resetRemoteWizard = () => {
    setSelectedRemoteProvider(null);
    setRemoteMessage(null);
    setRemoteAuthorizationUrl(null);
    remoteAuthSessionIdRef.current = null;
    setRemoteDisplayName(getRemoteProviderDisplayName(t, "google_drive"));
    setRemoteServerUrl("");
    setRemoteUsername("");
    setRemotePassword("");
    setRemoteRootPath("/OpenKara");
  };

  useEffect(() => {
    return () => {
      if (remoteAuthSessionIdRef.current) {
        void api.cancelRemoteAuth(remoteAuthSessionIdRef.current);
      }
    };
  }, []);

  const handleLanguageSelect = (code: string) => {
    setSelectedLanguageDraft(code);
    patchAppSettings({ language: code });
    i18next.changeLanguage(code);
    api
      .setLanguage(code)
      .then(hydrateAppSettings)
      .catch(() => {
        // non-fatal: language saved on next step anyway
      });
    setStep("library");
  };

  const handleCreate = async () => {
    const selected = await open({
      directory: true,
      title: t("setup.dialogTitleCreate"),
    });

    if (!selected) return;
    const selectedDirectory = resolveSingleDirectory(selected);
    if (!selectedDirectory) return;

    const libraryDir = `${selectedDirectory}/OpenKara`;
    setLoading(true);
    setError(null);
    try {
      await api.createLocalLibrary(libraryDir);
      setStep("stemMode");
    } catch (err: unknown) {
      setError(getErrorMessage(err));
    } finally {
      setLoading(false);
    }
  };

  const handleOpen = async () => {
    const selected = await open({
      directory: true,
      title: t("setup.dialogTitleOpen"),
    });

    if (!selected) return;
    const selectedDirectory = resolveSingleDirectory(selected);
    if (!selectedDirectory) return;

    setLoading(true);
    setError(null);
    try {
      await api.registerLocalLibrary(selectedDirectory);
      setStep("stemMode");
    } catch (err: unknown) {
      setError(getErrorMessage(err));
    } finally {
      setLoading(false);
    }
  };

  const handleOpenRemote = () => {
    resetRemoteWizard();
    setError(null);
    setStep("remoteProvider");
  };

  const connectRemoteLibrary = async (provider: RemoteLibraryProvider) => {
    setError(null);
    setRemoteMessage(null);
    setRemoteAuthorizationUrl(null);
    setSelectedRemoteProvider(provider);
    setLoading(true);
    try {
      await runRemoteLibraryRegistrationFlow({
        provider,
        displayName: remoteDisplayName,
        t,
        webdav: {
          serverUrl: remoteServerUrl,
          username: remoteUsername,
          password: remotePassword,
          rootPath: remoteRootPath,
        },
        onSessionIdChange: (sessionId) => {
          remoteAuthSessionIdRef.current = sessionId;
        },
        onAuthorizationUrlChange: setRemoteAuthorizationUrl,
        onMessageChange: setRemoteMessage,
      });
      setRemoteMessage(getRemoteLibraryConnectedMessage(t, provider));
      setStep("stemMode");
    } catch (err: unknown) {
      setError(getErrorMessage(err));
    } finally {
      setLoading(false);
    }
  };

  const handleWebDavConnect = async () => {
    await connectRemoteLibrary("webdav");
  };

  const handleGoogleDriveConnect = async () => {
    await connectRemoteLibrary("google_drive");
  };

  const handleDropboxConnect = async () => {
    await connectRemoteLibrary("dropbox");
  };

  const handleFinish = async () => {
    try {
      const settings = await api.setStemMode(selectedStemMode);
      hydrateAppSettings(settings);
    } catch {
      // non-fatal
    }
    onComplete();
  };

  return (
    <div className="flex h-screen w-full items-center justify-center bg-[var(--color-surface)]">
      <div className="mx-auto max-w-md space-y-8 px-6 text-center">
        <StepIndicator current={step} />

        {step === "language" && (
          <>
            <div className="flex flex-col items-center gap-4">
              <div className="flex items-center justify-center">
                <Globe
                  size={32}
                  className="text-[var(--color-control-primary)]"
                />
              </div>
              <h1 className="text-2xl font-bold text-[var(--color-text)]">
                {t("setup.chooseLanguage")}
              </h1>
            </div>

            <div className="space-y-3">
              {SUPPORTED_LANGUAGES.length > 6 && (
                <div className="relative">
                  <Search
                    size={16}
                    className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-[var(--color-text-dim)]"
                  />
                  <input
                    type="text"
                    value={languageFilter}
                    onChange={(event) => setLanguageFilter(event.target.value)}
                    placeholder={t("common.search")}
                    aria-label={t("common.search")}
                    className="w-full rounded-lg border border-[var(--color-border-light)] bg-[var(--color-sidebar)] py-2.5 pl-9 pr-3 text-[14px] text-[var(--color-text)] focus:border-[var(--color-accent)] focus:outline-none"
                  />
                </div>
              )}
              <div className="max-h-[60vh] space-y-3 overflow-y-auto [mask-image:linear-gradient(to_bottom,#000_calc(100%-2.5rem),transparent)]">
                {SUPPORTED_LANGUAGES.filter((lang) => {
                  const q = languageFilter.trim().toLowerCase();
                  const languageName = t(lang.nameKey);
                  if (!q) return true;
                  return (
                    languageName.toLowerCase().includes(q) ||
                    lang.code.toLowerCase().includes(q)
                  );
                }).map((lang) => (
                  <button
                    key={lang.code}
                    onClick={() => handleLanguageSelect(lang.code)}
                    className={`flex w-full items-center gap-3 rounded-lg border px-5 py-4 text-left transition-colors ${
                      selectedLanguage === lang.code
                        ? "border-[var(--color-control-selected-border)] bg-[var(--color-control-selected-bg)]"
                        : "border-[var(--color-border-light)] bg-[var(--color-sidebar)] hover:bg-[var(--color-hover)]"
                    }`}
                  >
                    <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-[var(--color-hover)]">
                      <span className="text-[14px] font-medium text-[var(--color-text)]">
                        {languageBadge(lang.code)}
                      </span>
                    </div>
                    <span className="text-[14px] font-medium text-[var(--color-text)]">
                      {t(lang.nameKey)}
                    </span>
                    {selectedLanguage === lang.code && (
                      <Check
                        size={16}
                        className="ml-auto text-[var(--color-control-primary)]"
                      />
                    )}
                  </button>
                ))}
              </div>
            </div>
          </>
        )}

        {step === "library" && (
          <>
            <div className="flex flex-col items-center gap-4">
              <div className="flex items-center justify-center">
                <Music
                  size={32}
                  className="text-[var(--color-control-primary)]"
                />
              </div>
              <h1 className="text-2xl font-bold text-[var(--color-text)]">
                {t("setup.welcome")}
              </h1>
              <p className="text-[14px] leading-relaxed text-[var(--color-text-dim)]">
                {t("setup.description")}
              </p>
            </div>

            <div className="space-y-3">
              {librarySetupChoices.map((choice) => {
                const Icon = choice.icon;
                const disabled = loading;

                return (
                  <button
                    key={choice.kind}
                    type="button"
                    aria-label={t(choice.title)}
                    onClick={
                      choice.kind === "create_local"
                        ? handleCreate
                        : choice.kind === "open_local"
                          ? handleOpen
                          : handleOpenRemote
                    }
                    disabled={disabled}
                    className="flex w-full items-start gap-3 rounded-lg border border-[var(--color-border-light)] bg-[var(--color-sidebar)] px-5 py-4 text-left transition-colors hover:bg-[var(--color-hover)] disabled:opacity-50"
                  >
                    <Icon
                      size={20}
                      className={`mt-0.5 shrink-0 ${
                        choice.kind === "open_remote"
                          ? "text-[var(--color-control-primary)]"
                          : choice.kind === "create_local"
                            ? "text-[var(--color-control-primary)]"
                            : "text-[var(--color-text-dim)]"
                      }`}
                    />
                    <div>
                      <div className="text-[14px] font-medium text-[var(--color-text)]">
                        {t(choice.title)}
                      </div>
                      <div className="text-[12px] text-[var(--color-text-dim)]">
                        {t(choice.description)}
                      </div>
                    </div>
                  </button>
                );
              })}
            </div>

            {error && (
              <p className="text-[13px] text-[var(--color-destructive)]">
                {error}
              </p>
            )}

            {loading && (
              <p className="text-[13px] text-[var(--color-text-dim)]">
                {t("setup.settingUp")}
              </p>
            )}

            <button
              onClick={() => setStep("language")}
              className="flex items-center justify-center gap-1 text-[13px] text-[var(--color-text-dim)] transition-colors hover:text-[var(--color-text)]"
            >
              <ChevronLeft size={14} />
              {t("setup.back")}
            </button>
          </>
        )}

        {step === "remoteProvider" && (
          <>
            <div className="flex flex-col items-center gap-4">
              <div className="flex items-center justify-center">
                <Cloud
                  size={32}
                  className="text-[var(--color-control-primary)]"
                />
              </div>
              <h1 className="text-2xl font-bold text-[var(--color-text)]">
                {t("setup.openRemoteLibrary")}
              </h1>
              <p className="text-[14px] leading-relaxed text-[var(--color-text-dim)]">
                {t("setup.openRemoteLibraryDescription")}
              </p>
            </div>

            <div className="space-y-3">
              {remoteLibraryProviders.map((choice) => {
                const Icon = choice.icon;
                const isActive = selectedRemoteProvider === choice.provider;

                return (
                  <button
                    key={choice.provider}
                    onClick={() => {
                      setSelectedRemoteProvider(choice.provider);
                      setRemoteMessage(null);
                      setError(null);
                      setRemoteAuthorizationUrl(null);
                      setRemoteDisplayName(
                        getRemoteProviderDisplayName(t, choice.provider),
                      );
                    }}
                    disabled={loading}
                    className={`flex w-full items-start gap-3 rounded-lg border px-5 py-4 text-left transition-colors disabled:opacity-50 ${
                      isActive
                        ? "border-[var(--color-control-selected-border)] bg-[var(--color-control-selected-bg)]"
                        : "border-[var(--color-border-light)] bg-[var(--color-sidebar)] hover:bg-[var(--color-hover)]"
                    }`}
                  >
                    <Icon
                      size={20}
                      className="mt-0.5 shrink-0 text-[var(--color-control-primary)]"
                    />
                    <div className="min-w-0 flex-1">
                      <div className="text-[14px] font-medium text-[var(--color-text)]">
                        {t(choice.title)}
                      </div>
                      <div className="text-[12px] text-[var(--color-text-dim)]">
                        {t(choice.description)}
                      </div>
                      <div className="mt-1 text-[11px] text-[var(--color-text-dimmer)]">
                        {choice.availableNow
                          ? t("setup.remoteProvider.availableNow")
                          : t("setup.remoteProvider.plannedLater")}
                      </div>
                    </div>
                    {isActive && (
                      <Check
                        size={16}
                        className="mt-0.5 shrink-0 text-[var(--color-control-primary)]"
                      />
                    )}
                  </button>
                );
              })}
            </div>

            {selectedRemoteProvider === "google_drive" && (
              <div className="space-y-3 rounded-lg border border-[var(--color-border-light)] bg-[var(--color-sidebar)] p-4 text-left">
                <div>
                  <label className="mb-1 block text-[12px] font-medium text-[var(--color-text)]">
                    {t("settings.library.displayName")}
                  </label>
                  <input
                    value={remoteDisplayName}
                    onChange={(event) =>
                      setRemoteDisplayName(event.target.value)
                    }
                    placeholder={getRemoteProviderDisplayName(
                      t,
                      "google_drive",
                    )}
                    className="w-full rounded-md border border-[var(--color-border-light)] bg-[var(--color-surface)] px-3 py-2 text-[13px] text-[var(--color-text)] outline-none transition-colors focus:border-[var(--color-accent)]"
                  />
                </div>
                <p className="text-[11px] text-[var(--color-text-dimmer)]">
                  {t("settings.library.googleDriveBundledDescription")}
                </p>

                <button
                  onClick={() => void handleGoogleDriveConnect()}
                  disabled={loading}
                  className="w-full rounded-lg bg-[var(--color-control-primary)] px-4 py-2.5 text-[13px] font-medium text-[var(--color-control-primary-foreground)] transition-opacity hover:opacity-90 disabled:opacity-60"
                >
                  {loading
                    ? t("settings.library.waitingForGoogle")
                    : t("settings.library.connectGoogleDrive")}
                </button>

                {remoteAuthorizationUrl && (
                  <a
                    href={remoteAuthorizationUrl}
                    target="_blank"
                    rel="noreferrer"
                    className="block text-[12px] text-[var(--color-accent)] underline underline-offset-2"
                  >
                    {t("settings.library.openGoogleBrowserSignInAgain")}
                  </a>
                )}
              </div>
            )}

            {selectedRemoteProvider === "webdav" && (
              <div className="space-y-3 rounded-lg border border-[var(--color-border-light)] bg-[var(--color-sidebar)] p-4 text-left">
                <div>
                  <label className="mb-1 block text-[12px] font-medium text-[var(--color-text)]">
                    {t("settings.library.displayName")}
                  </label>
                  <input
                    value={remoteDisplayName}
                    onChange={(event) =>
                      setRemoteDisplayName(event.target.value)
                    }
                    placeholder={getRemoteProviderDisplayName(t, "webdav")}
                    className="w-full rounded-md border border-[var(--color-border-light)] bg-[var(--color-surface)] px-3 py-2 text-[13px] text-[var(--color-text)] outline-none transition-colors focus:border-[var(--color-accent)]"
                  />
                </div>

                <div>
                  <label
                    htmlFor={WEBDAV_SERVER_URL_ID}
                    className="mb-1 block text-[12px] font-medium text-[var(--color-text)]"
                  >
                    {t("settings.library.webdavServerUrl")}
                  </label>
                  <input
                    id={WEBDAV_SERVER_URL_ID}
                    value={remoteServerUrl}
                    onChange={(event) => setRemoteServerUrl(event.target.value)}
                    className="w-full rounded-md border border-[var(--color-border-light)] bg-[var(--color-surface)] px-3 py-2 text-[13px] text-[var(--color-text)] outline-none transition-colors focus:border-[var(--color-accent)]"
                    spellCheck={false}
                  />
                </div>

                <div>
                  <label
                    htmlFor={WEBDAV_LIBRARY_PATH_ID}
                    className="mb-1 block text-[12px] font-medium text-[var(--color-text)]"
                  >
                    {t("settings.library.webdavLibraryPath")}
                  </label>
                  <input
                    id={WEBDAV_LIBRARY_PATH_ID}
                    value={remoteRootPath}
                    onChange={(event) => setRemoteRootPath(event.target.value)}
                    className="w-full rounded-md border border-[var(--color-border-light)] bg-[var(--color-surface)] px-3 py-2 text-[13px] text-[var(--color-text)] outline-none transition-colors focus:border-[var(--color-accent)]"
                    spellCheck={false}
                  />
                  <p className="mt-1 text-[11px] text-[var(--color-text-dimmer)]">
                    {t("settings.library.webdavLibraryPathDescription")}
                  </p>
                </div>

                <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
                  <div>
                    <label
                      htmlFor={WEBDAV_USERNAME_ID}
                      className="mb-1 block text-[12px] font-medium text-[var(--color-text)]"
                    >
                      {t("settings.library.webdavUsername")}
                    </label>
                    <input
                      id={WEBDAV_USERNAME_ID}
                      value={remoteUsername}
                      onChange={(event) =>
                        setRemoteUsername(event.target.value)
                      }
                      className="w-full rounded-md border border-[var(--color-border-light)] bg-[var(--color-surface)] px-3 py-2 text-[13px] text-[var(--color-text)] outline-none transition-colors focus:border-[var(--color-accent)]"
                      spellCheck={false}
                    />
                  </div>
                  <div>
                    <label
                      htmlFor={WEBDAV_PASSWORD_ID}
                      className="mb-1 block text-[12px] font-medium text-[var(--color-text)]"
                    >
                      {t("settings.library.webdavPassword")}
                    </label>
                    <input
                      id={WEBDAV_PASSWORD_ID}
                      type="password"
                      value={remotePassword}
                      onChange={(event) =>
                        setRemotePassword(event.target.value)
                      }
                      className="w-full rounded-md border border-[var(--color-border-light)] bg-[var(--color-surface)] px-3 py-2 text-[13px] text-[var(--color-text)] outline-none transition-colors focus:border-[var(--color-accent)]"
                    />
                  </div>
                </div>

                <button
                  onClick={() => void handleWebDavConnect()}
                  disabled={loading}
                  className="w-full rounded-lg bg-[var(--color-control-primary)] px-4 py-2.5 text-[13px] font-medium text-[var(--color-control-primary-foreground)] transition-opacity hover:opacity-90 disabled:opacity-60"
                >
                  {loading
                    ? t("settings.library.connecting")
                    : t("settings.library.connectWebdavLibrary")}
                </button>
              </div>
            )}

            {selectedRemoteProvider === "dropbox" && (
              <div className="space-y-3 rounded-lg border border-[var(--color-border-light)] bg-[var(--color-sidebar)] p-4 text-left">
                <div>
                  <label className="mb-1 block text-[12px] font-medium text-[var(--color-text)]">
                    {t("settings.library.displayName")}
                  </label>
                  <input
                    value={remoteDisplayName}
                    onChange={(event) =>
                      setRemoteDisplayName(event.target.value)
                    }
                    placeholder={getRemoteProviderDisplayName(t, "dropbox")}
                    className="w-full rounded-md border border-[var(--color-border-light)] bg-[var(--color-surface)] px-3 py-2 text-[13px] text-[var(--color-text)] outline-none transition-colors focus:border-[var(--color-accent)]"
                  />
                </div>
                <p className="text-[11px] text-[var(--color-text-dimmer)]">
                  {t("settings.library.dropboxBundledDescription")}
                </p>

                <button
                  onClick={() => void handleDropboxConnect()}
                  disabled={loading}
                  className="w-full rounded-lg bg-[var(--color-control-primary)] px-4 py-2.5 text-[13px] font-medium text-[var(--color-control-primary-foreground)] transition-opacity hover:opacity-90 disabled:opacity-60"
                >
                  {loading
                    ? t("settings.library.waitingForDropbox")
                    : t("settings.library.connectDropbox")}
                </button>

                {remoteAuthorizationUrl && (
                  <a
                    href={remoteAuthorizationUrl}
                    target="_blank"
                    rel="noreferrer"
                    className="block text-[12px] text-[var(--color-accent)] underline underline-offset-2"
                  >
                    {t("settings.library.openDropboxBrowserSignInAgain")}
                  </a>
                )}
              </div>
            )}

            {selectedRemoteProvider && remoteMessage && (
              <p className="text-[13px] text-[var(--color-text-dim)]">
                {remoteMessage}
              </p>
            )}

            {error && (
              <p className="text-[13px] text-[var(--color-destructive)]">
                {error}
              </p>
            )}

            <button
              onClick={() => {
                if (remoteAuthSessionIdRef.current) {
                  void api.cancelRemoteAuth(remoteAuthSessionIdRef.current);
                }
                resetRemoteWizard();
                setStep("library");
              }}
              className="flex items-center justify-center gap-1 text-[13px] text-[var(--color-text-dim)] transition-colors hover:text-[var(--color-text)]"
            >
              <ChevronLeft size={14} />
              {t("setup.back")}
            </button>
          </>
        )}

        {step === "stemMode" && (
          <>
            <div className="flex flex-col items-center gap-4">
              <div className="flex items-center justify-center">
                <Layers
                  size={32}
                  className="text-[var(--color-control-primary)]"
                />
              </div>
              <h1 className="text-2xl font-bold text-[var(--color-text)]">
                {t("setup.chooseStemMode")}
              </h1>
              <p className="text-[14px] leading-relaxed text-[var(--color-text-dim)]">
                {t("setup.stemModeDescription")}
              </p>
            </div>

            <div className="space-y-3">
              <button
                onClick={() => setSelectedStemModeDraft("two_stem")}
                className={`flex w-full items-start gap-3 rounded-lg border px-5 py-4 text-left transition-colors ${
                  selectedStemMode === "two_stem"
                    ? "border-[var(--color-control-selected-border)] bg-[var(--color-control-selected-bg)]"
                    : "border-[var(--color-border-light)] bg-[var(--color-sidebar)] hover:bg-[var(--color-hover)]"
                }`}
              >
                <Mic2
                  size={20}
                  className="mt-0.5 shrink-0 text-[var(--color-control-primary)]"
                />
                <div className="flex-1">
                  <div className="text-[14px] font-medium text-[var(--color-text)]">
                    {t("setup.twoStem")}
                  </div>
                  <div className="text-[12px] text-[var(--color-text-dim)]">
                    {t("setup.twoStemSubtitle")}
                  </div>
                  <div className="mt-0.5 text-[11px] text-[var(--color-text-dimmer)]">
                    {t("setup.twoStemDetail")}
                  </div>
                </div>
                {selectedStemMode === "two_stem" && (
                  <Check
                    size={16}
                    className="mt-0.5 shrink-0 text-[var(--color-control-primary)]"
                  />
                )}
              </button>

              <button
                onClick={() => setSelectedStemModeDraft("four_stem")}
                className={`flex w-full items-start gap-3 rounded-lg border px-5 py-4 text-left transition-colors ${
                  selectedStemMode === "four_stem"
                    ? "border-[var(--color-control-selected-border)] bg-[var(--color-control-selected-bg)]"
                    : "border-[var(--color-border-light)] bg-[var(--color-sidebar)] hover:bg-[var(--color-hover)]"
                }`}
              >
                <Layers
                  size={20}
                  className="mt-0.5 shrink-0 text-[var(--color-control-primary)]"
                />
                <div className="flex-1">
                  <div className="text-[14px] font-medium text-[var(--color-text)]">
                    {t("setup.fourStem")}
                  </div>
                  <div className="text-[12px] text-[var(--color-text-dim)]">
                    {t("setup.fourStemSubtitle")}
                  </div>
                  <div className="mt-0.5 text-[11px] text-[var(--color-text-dimmer)]">
                    {t("setup.fourStemDetail")}
                  </div>
                </div>
                {selectedStemMode === "four_stem" && (
                  <Check
                    size={16}
                    className="mt-0.5 shrink-0 text-[var(--color-control-primary)]"
                  />
                )}
              </button>
            </div>

            <p className="text-[12px] text-[var(--color-text-dimmer)]">
              {t("setup.modelDownloadHint")}
            </p>

            <button
              onClick={handleFinish}
              className="w-full rounded-lg bg-[var(--color-control-primary)] px-5 py-3 text-[14px] font-medium text-[var(--color-control-primary-foreground)] transition-opacity hover:opacity-90"
            >
              {t("setup.getStarted")}
            </button>

            <button
              onClick={() => setStep("library")}
              className="flex items-center justify-center gap-1 text-[13px] text-[var(--color-text-dim)] transition-colors hover:text-[var(--color-text)]"
            >
              <ChevronLeft size={14} />
              {t("setup.back")}
            </button>
          </>
        )}
      </div>
    </div>
  );
}
