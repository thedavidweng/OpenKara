import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";

interface EventSubscription {
  event: string;
  handler: (payload: unknown) => void;
}

export function useEventSubscriptions(
  subscriptions: EventSubscription[],
  enabled: boolean,
  onCleanup?: () => void,
  deps: React.DependencyList = [],
): void {
  useEffect(() => {
    if (!enabled) return;

    let cancelled = false;
    const unlisteners: (() => void)[] = [];

    const setup = async () => {
      for (const sub of subscriptions) {
        const unlisten = await listen(sub.event, (e) => {
          if (!cancelled) sub.handler(e.payload);
        });
        if (cancelled) {
          unlisten();
        } else {
          unlisteners.push(unlisten);
        }
      }
    };

    void setup();

    return () => {
      cancelled = true;
      onCleanup?.();
      unlisteners.forEach((fn) => fn());
    };
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [enabled, ...deps]);
}
