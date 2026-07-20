import {
  cloneElement,
  useCallback,
  useEffect,
  useLayoutEffect,
  useReducer,
  useRef,
  useState,
  useId,
  isValidElement,
  type ReactElement,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";
import {
  createTooltipScheduleController,
  getTooltipPosition,
  tooltipVisibilityReducer,
} from "./Tooltip.utils";
import { useTooltipDelayCoordinator } from "./Tooltip.context";
import {
  DEFAULT_DELAY_DURATION_MS,
  DEFAULT_HIDE_GRACE_DURATION_MS,
} from "./Tooltip.constants";

interface TooltipProps {
  children: ReactNode;
  label: string;
  shortcut?: string;
  /** Override provider delay for low-frequency explanatory tooltips. */
  delayDuration?: number;
  disabled?: boolean;
}

export function Tooltip({
  children,
  label,
  shortcut,
  delayDuration,
  disabled = false,
}: TooltipProps) {
  const coordinator = useTooltipDelayCoordinator();
  const anchorRef = useRef<HTMLSpanElement>(null);
  const tooltipRef = useRef<HTMLDivElement>(null);
  const scheduleRef = useRef<ReturnType<
    typeof createTooltipScheduleController
  > | null>(null);
  const openRef = useRef(false);
  const [open, dispatch] = useReducer(tooltipVisibilityReducer, false);
  const tooltipId = useId();
  const [position, setPosition] = useState<{
    left: number;
    top: number;
  } | null>(null);

  openRef.current = open;

  const resolvedDelayDuration = delayDuration ?? DEFAULT_DELAY_DURATION_MS;

  const showTooltip = useCallback(() => {
    if (openRef.current) {
      return;
    }
    dispatch({ type: "show" });
    coordinator.markOpened(tooltipId);
  }, [coordinator, tooltipId]);

  const hideTooltip = useCallback(() => {
    if (!openRef.current) {
      return;
    }
    dispatch({ type: "hide" });
    coordinator.markClosed();
  }, [coordinator]);

  const forceHideTooltip = useCallback(() => {
    scheduleRef.current?.cancelAll();
    if (!openRef.current) {
      return;
    }
    dispatch({ type: "hide" });
    coordinator.markClosed();
  }, [coordinator]);

  const getScheduleController = useCallback(() => {
    scheduleRef.current?.cancelAll();
    scheduleRef.current = createTooltipScheduleController({
      delayDuration: resolvedDelayDuration,
      hideGraceDuration: DEFAULT_HIDE_GRACE_DURATION_MS,
      skipDelay: coordinator.isSkipDelayActive(),
    });
    return scheduleRef.current;
  }, [coordinator, resolvedDelayDuration]);

  const describedChildren = (() => {
    if (!open) {
      return children;
    }

    if (!isValidElement(children)) {
      return children;
    }

    const existing = (children.props as { "aria-describedby"?: string })?.[
      "aria-describedby"
    ];
    const merged = existing
      ? existing.split(/\s+/).includes(tooltipId)
        ? existing
        : `${existing} ${tooltipId}`
      : tooltipId;

    return cloneElement(
      children as ReactElement,
      {
        "aria-describedby": merged,
      } as Record<string, unknown>,
    );
  })();

  useLayoutEffect(() => {
    if (
      disabled ||
      !open ||
      !anchorRef.current ||
      !tooltipRef.current ||
      typeof window === "undefined"
    ) {
      return;
    }

    const updatePosition = () => {
      if (!anchorRef.current || !tooltipRef.current) {
        return;
      }

      setPosition(
        getTooltipPosition(
          anchorRef.current.getBoundingClientRect(),
          {
            width: tooltipRef.current.offsetWidth,
            height: tooltipRef.current.offsetHeight,
          },
          {
            width: window.innerWidth,
            height: window.innerHeight,
          },
        ),
      );
    };

    updatePosition();
    window.addEventListener("resize", updatePosition);
    window.addEventListener("scroll", updatePosition, true);

    return () => {
      window.removeEventListener("resize", updatePosition);
      window.removeEventListener("scroll", updatePosition, true);
    };
  }, [disabled, open]);

  useEffect(() => {
    if (disabled || !open) {
      return;
    }

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        scheduleRef.current?.cancelAll();
        if (!openRef.current) {
          return;
        }
        dispatch({ type: "escape" });
        coordinator.markClosed();
      }
    };

    document.addEventListener("keydown", handleKeyDown);
    return () => {
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [coordinator, disabled, open]);

  useEffect(() => {
    if (disabled) {
      return;
    }

    coordinator.registerTooltip(tooltipId, forceHideTooltip);
    return () => {
      coordinator.unregisterTooltip(tooltipId);
      scheduleRef.current?.cancelAll();
      if (openRef.current) {
        coordinator.markClosed();
      }
    };
  }, [coordinator, disabled, forceHideTooltip, tooltipId]);

  if (disabled) {
    return <>{children}</>;
  }

  return (
    <>
      <span
        ref={anchorRef}
        className="-m-1 inline-flex p-1"
        onMouseEnter={() => {
          coordinator.cancelClose();
          getScheduleController().scheduleShow(showTooltip);
        }}
        onMouseLeave={() => {
          getScheduleController().scheduleHide(hideTooltip);
        }}
        onFocusCapture={() => {
          scheduleRef.current?.cancelAll();
          showTooltip();
        }}
        onBlurCapture={(event) => {
          if (anchorRef.current?.contains(event.relatedTarget as Node)) {
            return;
          }
          scheduleRef.current?.cancelAll();
          hideTooltip();
        }}
      >
        {describedChildren}
      </span>

      {open && typeof document !== "undefined"
        ? createPortal(
            <div
              ref={tooltipRef}
              role="tooltip"
              id={tooltipId}
              className="app-panel-surface pointer-events-none fixed z-[80] flex items-center gap-2 rounded-xl border border-[color-mix(in_srgb,var(--color-border)_88%,transparent)] bg-[color-mix(in_srgb,var(--color-sidebar)_96%,transparent)] px-3 py-1.5 text-[11px] font-medium text-[var(--color-text)] shadow-md"
              style={
                position
                  ? position
                  : {
                      left: 0,
                      top: 0,
                      opacity: 0,
                    }
              }
            >
              <span>{label}</span>
              {shortcut ? (
                <span className="rounded-md bg-[var(--color-ghost-hover)] px-1.5 py-0.5 text-[10px] font-semibold text-[var(--color-text-dim)]">
                  {shortcut}
                </span>
              ) : null}
            </div>,
            document.body,
          )
        : null}
    </>
  );
}

export { TooltipProvider } from "./TooltipProvider";
