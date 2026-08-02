import { useEffect } from "react";
import type { RuntimeEventSubscription } from "@/runtime/event-source";
import { createRuntimeEventRuntime } from "@/runtime/event-runtime";

export function useEventSubscriptions(
  subscriptions: RuntimeEventSubscription[],
  enabled: boolean,
  onCleanup?: () => void,
  deps: React.DependencyList = [],
): void {
  useEffect(() => {
    if (!enabled) return;

    const runtime = createRuntimeEventRuntime(subscriptions, onCleanup);
    void runtime.start();

    return () => {
      runtime.stop();
    };
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [enabled, ...deps]);
}
