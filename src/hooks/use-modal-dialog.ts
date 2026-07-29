import { useEffect, useRef, type RefObject } from "react";

const FOCUSABLE_SELECTOR = [
  "a[href]",
  "area[href]",
  "button:not([disabled])",
  "input:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  "[contenteditable='true']",
  "[tabindex]:not([tabindex='-1'])",
].join(",");

let nextDialogId = 0;
const activeDialogIds: number[] = [];

interface UseModalDialogOptions {
  dialogRef: RefObject<HTMLElement | null>;
  initialFocusRef?: RefObject<HTMLElement | null>;
  onDismiss: () => void;
  canDismiss?: boolean;
  enabled?: boolean;
  selectInitialText?: boolean;
}

function focusableElements(container: HTMLElement): HTMLElement[] {
  return Array.from(
    container.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR),
  ).filter(
    (element) =>
      !element.hasAttribute("hidden") &&
      element.getAttribute("aria-hidden") !== "true",
  );
}

export function useModalDialog({
  dialogRef,
  initialFocusRef,
  onDismiss,
  canDismiss = true,
  enabled = true,
  selectInitialText = false,
}: UseModalDialogOptions): void {
  const dismissRef = useRef(onDismiss);
  const canDismissRef = useRef(canDismiss);

  dismissRef.current = onDismiss;
  canDismissRef.current = canDismiss;

  useEffect(() => {
    if (!enabled || typeof document === "undefined") {
      return;
    }

    const dialogId = ++nextDialogId;
    activeDialogIds.push(dialogId);
    const previousFocus =
      document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null;

    const focusInitialElement = () => {
      const dialog = dialogRef.current;
      if (!dialog) return;

      const initial = initialFocusRef?.current ?? focusableElements(dialog)[0];
      if (initial) {
        initial.focus({ preventScroll: true });
        if (selectInitialText && initial instanceof HTMLInputElement) {
          initial.select();
        }
        return;
      }

      dialog.focus({ preventScroll: true });
    };

    const animationFrame = window.requestAnimationFrame(focusInitialElement);
    const handleKeyDown = (event: KeyboardEvent) => {
      if (activeDialogIds[activeDialogIds.length - 1] !== dialogId) return;

      if (event.key === "Escape" && canDismissRef.current) {
        event.preventDefault();
        event.stopPropagation();
        dismissRef.current();
        return;
      }

      if (event.key !== "Tab") return;

      const dialog = dialogRef.current;
      if (!dialog) return;

      const focusable = focusableElements(dialog);
      if (focusable.length === 0) {
        event.preventDefault();
        dialog.focus({ preventScroll: true });
        return;
      }

      const activeElement = document.activeElement as HTMLElement | null;
      const currentIndex = activeElement
        ? focusable.indexOf(activeElement)
        : -1;
      const first = focusable[0];
      const last = focusable[focusable.length - 1];

      if (event.shiftKey) {
        if (currentIndex <= 0) {
          event.preventDefault();
          last.focus({ preventScroll: true });
        }
      } else if (currentIndex === -1 || currentIndex === focusable.length - 1) {
        event.preventDefault();
        first.focus({ preventScroll: true });
      }
    };

    document.addEventListener("keydown", handleKeyDown);
    return () => {
      window.cancelAnimationFrame(animationFrame);
      document.removeEventListener("keydown", handleKeyDown);
      const index = activeDialogIds.lastIndexOf(dialogId);
      if (index >= 0) activeDialogIds.splice(index, 1);

      window.requestAnimationFrame(() => {
        if (previousFocus?.isConnected) {
          previousFocus.focus({ preventScroll: true });
        }
      });
    };
  }, [dialogRef, enabled, initialFocusRef, selectInitialText]);
}
