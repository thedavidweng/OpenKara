import { useEventSubscriptions } from "@/hooks/use-event-subscription";
import { useBootstrapStore } from "@/stores/bootstrap-store";
import { useRuntimeBootstrapStore } from "@/stores/runtime-bootstrap-store";
import {
  eventSubscription,
  tauriRuntimeEventSource,
  type RuntimeEventSource,
} from "@/runtime/event-source";

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
        updateRuntimeBootstrapStatus,
        source,
      ),
      eventSubscription(
        "runtime-bootstrap-ready",
        updateRuntimeBootstrapStatus,
        source,
      ),
      eventSubscription(
        "runtime-bootstrap-error",
        updateRuntimeBootstrapStatus,
        source,
      ),
    ],
    enabled,
    undefined,
    [source, updateBootstrapStatus, updateRuntimeBootstrapStatus],
  );
}
