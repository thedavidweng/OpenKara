import { useEffect, type DependencyList } from "react";

/**
 * Adds a Escape key listener to `window`. For popovers that combine Escape
 * with click-outside on `document`, keep the inline useEffect instead —
 * splitting the listeners would add a second effect for no gain.
 */
export function useEscapeKey(onEscape: () => void, deps: DependencyList = []) {
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") onEscape();
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);
}
