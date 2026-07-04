import { createContext, useContext } from "react";

export interface TooltipProviderConfig {
  delayDuration: number;
  skipDelayDuration: number;
  hideGraceDuration: number;
}

export interface TooltipDelayCoordinator {
  config: TooltipProviderConfig;
  isSkipDelayActive: () => boolean;
  markOpened: (id: string) => void;
  markClosed: () => void;
  cancelClose: () => void;
  registerTooltip: (id: string, forceHide: () => void) => void;
  unregisterTooltip: (id: string) => void;
}

const DEFAULT_DELAY_DURATION_MS = 600;
const DEFAULT_SKIP_DELAY_DURATION_MS = 300;
const DEFAULT_HIDE_GRACE_DURATION_MS = 120;

export const TooltipDelayContext =
  createContext<TooltipDelayCoordinator | null>(null);

export function useTooltipDelayCoordinator(): TooltipDelayCoordinator {
  const coordinator = useContext(TooltipDelayContext);
  if (!coordinator) {
    return {
      config: {
        delayDuration: DEFAULT_DELAY_DURATION_MS,
        skipDelayDuration: DEFAULT_SKIP_DELAY_DURATION_MS,
        hideGraceDuration: DEFAULT_HIDE_GRACE_DURATION_MS,
      },
      isSkipDelayActive: () => false,
      registerTooltip: () => {},
      unregisterTooltip: () => {},
      markOpened: () => {},
      markClosed: () => {},
      cancelClose: () => {},
    };
  }
  return coordinator;
}
