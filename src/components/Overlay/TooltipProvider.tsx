import { useCallback, useEffect, useRef, useMemo, type ReactNode } from "react";
import {
  TooltipDelayContext,
  type TooltipDelayCoordinator,
} from "./Tooltip.context";
import { DEFAULT_SKIP_DELAY_DURATION_MS } from "./Tooltip.constants";

export interface TooltipProviderProps {
  children: ReactNode;
}

export function TooltipProvider({ children }: TooltipProviderProps) {
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

  const scheduleSkipDelayReset = useCallback(() => {
    clearSkipDelayTimer();
    skipDelayTimerRef.current = setTimeout(() => {
      skipDelayActiveRef.current = false;
      skipDelayTimerRef.current = null;
    }, DEFAULT_SKIP_DELAY_DURATION_MS);
  }, [clearSkipDelayTimer]);

  const coordinator = useMemo<TooltipDelayCoordinator>(
    () => ({
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
          scheduleSkipDelayReset();
        }
      },
      cancelClose: () => {
        clearSkipDelayTimer();
      },
    }),
    [clearSkipDelayTimer, scheduleSkipDelayReset],
  );

  useEffect(() => {
    const tooltipRegistry = tooltipRegistryRef;
    return () => {
      clearSkipDelayTimer();
      openCountRef.current = 0;
      skipDelayActiveRef.current = false;
      tooltipRegistry.current.clear();
    };
  }, [clearSkipDelayTimer]);

  return (
    <TooltipDelayContext.Provider value={coordinator}>
      {children}
    </TooltipDelayContext.Provider>
  );
}
