import { useCallback, useMemo, useRef, type ReactNode } from "react";
import {
  TooltipDelayContext,
  type TooltipDelayCoordinator,
  type TooltipProviderConfig,
} from "./Tooltip.context";

export interface TooltipProviderProps {
  children: ReactNode;
  /** First-hover delay before a tooltip appears. */
  delayDuration?: number;
  /** After the last tooltip closes, instant switching stays enabled for this long. */
  skipDelayDuration?: number;
  /** Brief grace before hiding so the pointer can cross gaps between adjacent triggers. */
  hideGraceDuration?: number;
}

const DEFAULT_DELAY_DURATION_MS = 600;
const DEFAULT_SKIP_DELAY_DURATION_MS = 300;
const DEFAULT_HIDE_GRACE_DURATION_MS = 120;

export function TooltipProvider({
  children,
  delayDuration = DEFAULT_DELAY_DURATION_MS,
  skipDelayDuration = DEFAULT_SKIP_DELAY_DURATION_MS,
  hideGraceDuration = DEFAULT_HIDE_GRACE_DURATION_MS,
}: TooltipProviderProps) {
  const skipDelayActiveRef = useRef(false);
  const openCountRef = useRef(0);
  const skipDelayTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const tooltipRegistryRef = useRef<Map<string, () => void>>(new Map());

  const clearSkipDelayTimer = useCallback(() => {
    if (skipDelayTimerRef.current) {
      clearTimeout(skipDelayTimerRef.current);
      skipDelayTimerRef.current = null;
    }
  }, []);

  const scheduleSkipDelayReset = useCallback(
    (config: TooltipProviderConfig) => {
      clearSkipDelayTimer();
      skipDelayTimerRef.current = setTimeout(() => {
        skipDelayActiveRef.current = false;
        skipDelayTimerRef.current = null;
      }, config.skipDelayDuration);
    },
    [clearSkipDelayTimer],
  );

  const config = useMemo<TooltipProviderConfig>(
    () => ({
      delayDuration,
      skipDelayDuration,
      hideGraceDuration,
    }),
    [delayDuration, hideGraceDuration, skipDelayDuration],
  );

  const coordinator = useMemo<TooltipDelayCoordinator>(
    () => ({
      config,
      isSkipDelayActive: () => skipDelayActiveRef.current,
      registerTooltip: (id, forceHide) => {
        tooltipRegistryRef.current.set(id, forceHide);
      },
      unregisterTooltip: (id) => {
        tooltipRegistryRef.current.delete(id);
      },
      markOpened: (id) => {
        for (const [otherId, forceHide] of tooltipRegistryRef.current) {
          if (otherId !== id) {
            forceHide();
          }
        }
        openCountRef.current += 1;
        skipDelayActiveRef.current = true;
        clearSkipDelayTimer();
      },
      markClosed: () => {
        openCountRef.current = Math.max(0, openCountRef.current - 1);
        if (openCountRef.current === 0) {
          scheduleSkipDelayReset(config);
        }
      },
      cancelClose: () => {
        clearSkipDelayTimer();
      },
    }),
    [clearSkipDelayTimer, config, scheduleSkipDelayReset],
  );

  return (
    <TooltipDelayContext.Provider value={coordinator}>
      {children}
    </TooltipDelayContext.Provider>
  );
}
