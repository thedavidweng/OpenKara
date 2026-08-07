import { useEventSubscriptions } from "@/hooks/use-event-subscription";
import { useBootstrapStore } from "@/stores/bootstrap-store";
import { useRuntimeBootstrapStore } from "@/stores/runtime-bootstrap-store";
import { useNotificationStore } from "@/stores/notification-store";
import {
  eventSubscription,
  tauriRuntimeEventSource,
  type RuntimeEventSource,
} from "@/runtime/event-source";
import type { RuntimeBootstrapStatusSnapshot } from "@/types/ipc";
import i18next from "@/lib/i18n";

const CPU_FALLBACK_NOTICE_STORAGE_KEY = "openkara.cpuFallbackNoticeShown";

// In-memory session flag mirrors the persisted guard. When localStorage throws
// (private contexts, quota), the session flag still suppresses repeat toasts
// for the rest of this app session.
let cpuFallbackNoticeShownThisSession = false;

function hasShownCpuFallbackNotice(): boolean {
  if (cpuFallbackNoticeShownThisSession) {
    return true;
  }
  try {
    return localStorage.getItem(CPU_FALLBACK_NOTICE_STORAGE_KEY) === "1";
  } catch {
    return false;
  }
}

function markCpuFallbackNoticeShown() {
  cpuFallbackNoticeShownThisSession = true;
  try {
    localStorage.setItem(CPU_FALLBACK_NOTICE_STORAGE_KEY, "1");
  } catch {
    // localStorage can be unavailable; the session flag above still suppresses
    // repeat toasts until the app restarts.
  }
}

function handleRuntimeBootstrapStatus(
  update: (incoming: RuntimeBootstrapStatusSnapshot) => void,
) {
  return (payload: RuntimeBootstrapStatusSnapshot) => {
    update(payload);
    if (payload.cpu_fallback_notice && !hasShownCpuFallbackNotice()) {
      markCpuFallbackNoticeShown();
      useNotificationStore.getState().addNotification({
        type: "info",
        title: i18next.t("settings.runtime.cpuFallbackToast.title"),
        message: i18next.t("settings.runtime.cpuFallbackToast.message"),
        retryable: false,
        dismissAfterMs: 12000,
      });
    }
  };
}

export function useBootstrapEvents(
  enabled: boolean,
  source: RuntimeEventSource = tauriRuntimeEventSource,
) {
  const updateBootstrapStatus = useBootstrapStore(
    (state) => state.updateStatus,
  );
  const updateRuntimeBootstrapStatus = useRuntimeBootstrapStore(
    (state) => state.updateStatus,
  );

  const handleRuntimeBootstrap = handleRuntimeBootstrapStatus(
    updateRuntimeBootstrapStatus,
  );

  useEventSubscriptions(
    [
      eventSubscription(
        "model-bootstrap-progress",
        updateBootstrapStatus,
        source,
      ),
      eventSubscription("model-bootstrap-ready", updateBootstrapStatus, source),
      eventSubscription("model-bootstrap-error", updateBootstrapStatus, source),
      eventSubscription(
        "runtime-bootstrap-progress",
        handleRuntimeBootstrap,
        source,
      ),
      eventSubscription(
        "runtime-bootstrap-ready",
        handleRuntimeBootstrap,
        source,
      ),
      eventSubscription(
        "runtime-bootstrap-error",
        handleRuntimeBootstrap,
        source,
      ),
    ],
    enabled,
    undefined,
    [source, updateBootstrapStatus, handleRuntimeBootstrap],
  );
}
