// @vitest-environment jsdom

import { act, useEffect } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test } from "vitest";
import {
  useTooltipDelayCoordinator,
  type TooltipDelayCoordinator,
} from "./Tooltip.context";
import {
  DEFAULT_DELAY_DURATION_MS,
  DEFAULT_HIDE_GRACE_DURATION_MS,
  DEFAULT_SKIP_DELAY_DURATION_MS,
} from "./Tooltip.constants";

describe("useTooltipDelayCoordinator", () => {
  let container: HTMLDivElement;
  let coordinator: TooltipDelayCoordinator | null = null;

  beforeEach(() => {
    (
      globalThis as typeof globalThis & {
        IS_REACT_ACT_ENVIRONMENT?: boolean;
      }
    ).IS_REACT_ACT_ENVIRONMENT = true;

    container = document.createElement("div");
    document.body.appendChild(container);
    coordinator = null;
  });

  afterEach(() => {
    container.remove();
  });

  test("returns a no-op fallback coordinator outside a provider", () => {
    function Probe() {
      const value = useTooltipDelayCoordinator();
      useEffect(() => {
        coordinator = value;
      }, [value]);
      return null;
    }

    const root = createRoot(container);
    act(() => {
      root.render(<Probe />);
    });

    expect(coordinator?.config).toEqual({
      delayDuration: DEFAULT_DELAY_DURATION_MS,
      skipDelayDuration: DEFAULT_SKIP_DELAY_DURATION_MS,
      hideGraceDuration: DEFAULT_HIDE_GRACE_DURATION_MS,
    });
    expect(coordinator?.isSkipDelayActive()).toBe(false);

    expect(() => {
      coordinator?.registerTooltip("tooltip-1", () => {});
      coordinator?.markOpened("tooltip-1");
      coordinator?.markClosed();
      coordinator?.cancelClose();
      coordinator?.unregisterTooltip("tooltip-1");
    }).not.toThrow();

    expect(coordinator?.isSkipDelayActive()).toBe(false);

    act(() => {
      root.unmount();
    });
  });

  test("returns the same fallback coordinator across re-renders", () => {
    function Probe() {
      const value = useTooltipDelayCoordinator();
      useEffect(() => {
        coordinator = value;
      }, [value]);
      return null;
    }

    const root = createRoot(container);
    act(() => {
      root.render(<Probe />);
    });
    const firstCoordinator = coordinator;

    act(() => {
      root.render(<Probe />);
    });

    expect(coordinator).toBe(firstCoordinator);

    act(() => {
      root.unmount();
    });
  });
});
