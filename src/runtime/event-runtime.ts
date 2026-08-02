import type { RuntimeEventSubscription } from "@/runtime/event-source";

export interface RuntimeEventRuntime {
  start(): Promise<void>;
  stop(): void;
}

export function createRuntimeEventRuntime(
  subscriptions: RuntimeEventSubscription[],
  onStop?: () => void,
): RuntimeEventRuntime {
  let active = false;
  let runId = 0;
  const unlisteners = new Set<() => void>();

  return {
    async start() {
      if (active) return;
      active = true;
      const currentRunId = ++runId;

      for (const subscription of subscriptions) {
        const unlisten = await subscription.subscribe();
        if (!active || currentRunId !== runId) {
          unlisten();
          return;
        }
        unlisteners.add(unlisten);
      }
    },

    stop() {
      if (!active) return;
      active = false;
      runId += 1;
      unlisteners.forEach((unlisten) => unlisten());
      unlisteners.clear();
      onStop?.();
    },
  };
}
