import { createContext, useContext } from "react";
import {
  DEFAULT_DELAY_DURATION_MS,
  DEFAULT_HIDE_GRACE_DURATION_MS,
  DEFAULT_SKIP_DELAY_DURATION_MS,
} from "./Tooltip.constants";

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

const FALLBACK_TOOLTIP_COORDINATOR: TooltipDelayCoordinator = {
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

export const TooltipDelayContext =
  createContext<TooltipDelayCoordinator | null>(null);

export function useTooltipDelayCoordinator(): TooltipDelayCoordinator {
  const coordinator = useContext(TooltipDelayContext);
  return coordinator ?? FALLBACK_TOOLTIP_COORDINATOR;
}
