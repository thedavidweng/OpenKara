import { createContext, useContext } from "react";

export interface TooltipDelayCoordinator {
  isSkipDelayActive: () => boolean;
  markOpened: (id: string) => void;
  markClosed: () => void;
  cancelClose: () => void;
  registerTooltip: (id: string, forceHide: () => void) => void;
  unregisterTooltip: (id: string) => void;
}

const FALLBACK_TOOLTIP_COORDINATOR: TooltipDelayCoordinator = {
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
