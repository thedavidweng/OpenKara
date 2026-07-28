import { useCallback, useRef } from "react";
import type {
  KeyboardEvent as ReactKeyboardEvent,
  PointerEvent as ReactPointerEvent,
  ReactNode,
} from "react";
import { MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH } from "@/stores/layout-store";

const SIDEBAR_KEYBOARD_RESIZE_STEP_PX = 16;

interface SidebarRailProps {
  visible: boolean;
  width: number;
  onResize: (width: number) => void;
  resizable?: boolean;
  children: ReactNode;
}

function clampWidth(width: number): number {
  return Math.min(MAX_SIDEBAR_WIDTH, Math.max(MIN_SIDEBAR_WIDTH, width));
}

export function SidebarRail({
  visible,
  width,
  onResize,
  resizable = true,
  children,
}: SidebarRailProps) {
  const dragStateRef = useRef<{ startX: number; startWidth: number } | null>(
    null,
  );

  const handleDragStart = useCallback(
    (event: ReactPointerEvent<HTMLDivElement>) => {
      if (!visible) {
        return;
      }

      const handle = event.currentTarget;
      dragStateRef.current = {
        startX: event.clientX,
        startWidth: width,
      };

      const onPointerMove = (moveEvent: PointerEvent) => {
        const dragState = dragStateRef.current;
        if (!dragState) {
          return;
        }

        const delta = moveEvent.clientX - dragState.startX;
        onResize(clampWidth(Math.round(dragState.startWidth + delta)));
      };

      const onPointerUp = () => {
        dragStateRef.current = null;
        window.removeEventListener("pointermove", onPointerMove);
        window.removeEventListener("pointerup", onPointerUp);
      };

      window.addEventListener("pointermove", onPointerMove);
      window.addEventListener("pointerup", onPointerUp, { once: true });
      handle.setPointerCapture(event.pointerId);
    },
    [onResize, visible, width],
  );

  const handleKeyboardResize = useCallback(
    (event: ReactKeyboardEvent<HTMLDivElement>) => {
      if (!visible) {
        return;
      }

      let nextWidth: number | null = null;
      switch (event.key) {
        case "ArrowLeft":
          nextWidth = width - SIDEBAR_KEYBOARD_RESIZE_STEP_PX;
          break;
        case "ArrowRight":
          nextWidth = width + SIDEBAR_KEYBOARD_RESIZE_STEP_PX;
          break;
        case "Home":
          nextWidth = MIN_SIDEBAR_WIDTH;
          break;
        case "End":
          nextWidth = MAX_SIDEBAR_WIDTH;
          break;
        default:
          return;
      }

      event.preventDefault();
      onResize(clampWidth(nextWidth));
    },
    [onResize, visible, width],
  );

  return (
    <div
      className={`shrink-0 overflow-hidden transition-[width] ${
        visible ? "w-[var(--window-shell-sidebar-width)] select-none" : "w-0"
      }`}
      style={{
        transitionDuration: "var(--motion-duration-standard)",
        transitionTimingFunction: "var(--motion-ease-emphasized-out)",
      }}
    >
      <div
        className={`relative h-full w-[var(--window-shell-sidebar-width)] transition-[opacity,transform] ${
          visible ? "translate-x-0 opacity-100" : "-translate-x-3 opacity-0"
        }`}
        style={{
          transitionDuration: "var(--motion-duration-standard)",
          transitionTimingFunction: "var(--motion-ease-emphasized-out)",
        }}
      >
        {children}
        {visible && resizable ? (
          <div
            role="separator"
            aria-orientation="vertical"
            aria-label="Resize sidebar"
            aria-valuemin={MIN_SIDEBAR_WIDTH}
            aria-valuemax={MAX_SIDEBAR_WIDTH}
            aria-valuenow={width}
            aria-valuetext={`${width} px`}
            tabIndex={0}
            onPointerDown={handleDragStart}
            onKeyDown={handleKeyboardResize}
            className="absolute right-0 top-0 h-full w-1.5 cursor-col-resize bg-transparent hover:bg-[var(--color-ghost-hover)] focus-visible:bg-[var(--color-ghost-hover)] focus-visible:outline-2 focus-visible:outline-[var(--color-focus-ring)]"
          />
        ) : null}
      </div>
    </div>
  );
}
