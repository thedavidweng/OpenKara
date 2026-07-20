import { useCallback, useReducer } from "react";
import { QueuePanel } from "@/components/Player/QueuePanel";
import { GlobalProgressBar } from "@/components/Layout/GlobalProgressBar";
import { PlaybackStage } from "@/components/Playback/PlaybackStage";
import { SettingsOverlay } from "@/components/Settings/SettingsOverlay";
import { ModelBootstrapBanner } from "@/components/Bootstrap/ModelBootstrapBanner";
import { PlaybackBar } from "@/components/Player/PlaybackBar";
import { useSettingsStore } from "@/stores/settings-store";
import { useQueueStore } from "@/stores/queue-store";

interface MainContentViewProps {
  previewMode?: boolean;
}

type QueuePhase = "visible" | "exiting" | "hidden";
type QueueAction = { type: "show" } | { type: "hide" } | { type: "exited" };

function queueReducer(_state: QueuePhase, action: QueueAction): QueuePhase {
  switch (action.type) {
    case "show":
      return "visible";
    case "hide":
      return "exiting";
    case "exited":
      return "hidden";
  }
}

export function MainContentView({
  previewMode = false,
}: MainContentViewProps = {}) {
  const settingsOpen = useSettingsStore((s) => s.isOpen);
  const queueOpen = useQueueStore((s) => s.isOpen);
  const [queuePhase, dispatch] = useReducer(
    queueReducer,
    queueOpen ? "visible" : "hidden",
  );

  if (queueOpen && queuePhase !== "visible") {
    dispatch({ type: "show" });
  } else if (!queueOpen && queuePhase === "visible") {
    dispatch({ type: "hide" });
  }

  const onQueueAnimationEnd = useCallback(() => {
    dispatch({ type: "exited" });
  }, []);

  const queueShouldRender = queuePhase !== "hidden";
  const queueClassName =
    queuePhase === "exiting"
      ? "animate-slide-out-right"
      : "animate-slide-in-right";

  return (
    <div
      className="flex min-w-0 flex-1 flex-col overflow-hidden bg-[var(--color-sidebar)]"
      data-main-content-visual-variant="unified"
    >
      {/* Stage pocket: color edge only (no border/shadow); chrome shows through soft arcs. */}
      <div
        className={`relative flex min-h-0 flex-1 overflow-hidden ${settingsOpen ? "bg-[var(--color-surface-muted)]" : "bg-[var(--color-surface)]"}`}
        data-shell-content-pocket="true"
      >
        <div className="flex min-w-0 flex-1 flex-col overflow-hidden">
          <ModelBootstrapBanner />
          <PlaybackStage />
        </div>
        {queueShouldRender && (
          <div
            className={`h-full ${queueClassName}`}
            onAnimationEnd={
              queuePhase === "exiting" ? onQueueAnimationEnd : undefined
            }
          >
            <QueuePanel />
          </div>
        )}
        {settingsOpen ? <SettingsOverlay /> : null}
      </div>

      {!previewMode && <GlobalProgressBar />}
      <PlaybackBar previewMode={previewMode} />
    </div>
  );
}
