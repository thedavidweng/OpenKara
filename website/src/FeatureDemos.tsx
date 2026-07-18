import type { ReactNode } from "react";
import { TooltipProvider } from "@/components/Overlay/Tooltip";
import { PlaybackStage } from "@/components/Playback/PlaybackStage";
import { PlaybackBar } from "@/components/Player/PlaybackBar";
import { QueuePanel } from "@/components/Player/QueuePanel";
import { VolumeSliders } from "@/components/Player/VolumeSliders";
import { SettingsOverlay } from "@/components/Settings/SettingsOverlay";
import type { SettingsOverlaySnapshot } from "@/components/Settings/SettingsOverlay.controller";
import { Toolbar } from "@/components/Layout/Toolbar";
import { getNativeWindowShellState } from "@/lib/window-shell";
import { initializeMockApp } from "./mock-app";

export type FeatureDemoKind = "mixer" | "rotation" | "settings";

interface FeatureDemoProps {
  kind: FeatureDemoKind;
  language: "en" | "zh-CN";
}

interface FeatureWindowProps {
  children: ReactNode;
  variant: FeatureDemoKind;
}

const PREVIEW_SHELL_STATE = getNativeWindowShellState();

function FeatureWindow({ children, variant }: FeatureWindowProps) {
  return (
    <div
      className={`feature-demo-window feature-demo-window--${variant}`}
      data-feature-demo-window={variant}
    >
      <Toolbar
        onToggleSidebar={() => {}}
        onToggleSettings={() => {}}
        previewMode
        shellState={PREVIEW_SHELL_STATE}
        settingsOpen={false}
        sidebarVisible
      />
      <div className="feature-demo-window-content">{children}</div>
    </div>
  );
}

function MixerDemo() {
  return (
    <FeatureWindow variant="mixer">
      <div className="feature-demo-playback-stage">
        <PlaybackStage />
        <div className="feature-demo-mixer-control">
          <VolumeSliders />
        </div>
      </div>
      <PlaybackBar densityOverride="compact" />
    </FeatureWindow>
  );
}

function RotationDemo() {
  return (
    <FeatureWindow variant="rotation">
      <div className="feature-demo-rotation-layout">
        <div className="feature-demo-playback-stage">
          <PlaybackStage />
        </div>
        <QueuePanel />
      </div>
      <PlaybackBar densityOverride="compact" />
    </FeatureWindow>
  );
}

function createSettingsSnapshot(
  language: FeatureDemoProps["language"],
): SettingsOverlaySnapshot {
  const library = {
    id: "openkara-library",
    kind: "local" as const,
    display_name: "OpenKara Library",
    root_path: "/Users/you/Music/OpenKara",
  };

  return {
    state: {
      libraryPath: library.root_path,
      libraryError: null,
      libraryRegistry: {
        active_library_id: library.id,
        libraries: [library],
      },
      libraries: [library],
      activeLibraryId: library.id,
      stemMode: "four_stem",
      modelVariant: "htdemucs_ft",
      modelStatuses: {
        htdemucs: {
          downloaded: true,
          legacy_install_present: false,
          file_size: 664_000_000,
        },
        htdemucs_ft: {
          downloaded: true,
          legacy_install_present: false,
          file_size: 1_028_000_000,
        },
      },
      downloadingModel: null,
      runtimeStatus: {
        state: "ready",
        version: "1.26.0",
        runtime_path: "/mock/runtime",
      },
      language,
      hideBatchSeparate: false,
      coverArtBackdrop: false,
      executionProvider: "xnnpack",
      availableExecutionProviders: ["cpu", "xnnpack"],
    },
    meta: {
      isInitializing: false,
      dangerDialog: null,
      stemsSize: null,
      downgradeSavings: null,
      deletingStemsInProgress: false,
      deletingLyricsInProgress: false,
      downgradingInProgress: false,
    },
  };
}

function SettingsDemo({ language }: Pick<FeatureDemoProps, "language">) {
  return (
    <FeatureWindow variant="settings">
      <div className="feature-demo-settings-stage">
        <SettingsOverlay
          initialSnapshot={createSettingsSnapshot(language)}
          skipInitialize
        />
      </div>
      <PlaybackBar densityOverride="compact" />
    </FeatureWindow>
  );
}

export function FeatureDemo({ kind, language }: FeatureDemoProps) {
  // The primary preview and every supporting panel share one deterministic
  // mock data set. The components below are real app UI, not screenshot art.
  initializeMockApp(language);

  const content =
    kind === "mixer" ? (
      <MixerDemo />
    ) : kind === "rotation" ? (
      <RotationDemo />
    ) : (
      <SettingsDemo language={language} />
    );

  return (
    <section
      className={`feature-demo feature-demo--${kind}`}
      data-feature-demo={kind}
      aria-label={
        kind === "mixer"
          ? "OpenKara stem mixer preview"
          : kind === "rotation"
            ? "OpenKara singer rotation preview"
            : "OpenKara settings preview"
      }
    >
      <TooltipProvider>
        <div data-feature-demo-static="true" aria-hidden="true" inert>
          {content}
        </div>
      </TooltipProvider>
    </section>
  );
}
