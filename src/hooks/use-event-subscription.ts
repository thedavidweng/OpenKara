import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";

interface EventSubscription {
  event: string;
  handler: (payload: unknown) => void;
}

/**
 * Optional `onCleanup` runs before unlisteners are called (for clearing
 * scheduled timers, etc.).
 */
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
    // subscriptions and onCleanup are intentionally excluded — subscriptions is
    // a new array each render and onCleanup identity may change without requiring
    // re-subscription (callers reach side-effects through stable refs). The deps
    // parameter gives callers explicit control over when to re-subscribe.
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [enabled, ...deps]);
}
