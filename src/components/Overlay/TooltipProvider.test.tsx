// @vitest-environment jsdom

import { act, useEffect } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import {
  useTooltipDelayCoordinator,
  type TooltipDelayCoordinator,
} from "./Tooltip.context";
import { TooltipProvider, type TooltipProviderProps } from "./TooltipProvider";

describe("TooltipProvider", () => {
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

  function renderProvider(props?: Omit<TooltipProviderProps, "children">) {
    function Probe() {
      const value = useTooltipDelayCoordinator();
      useEffect(() => {
        coordinator = value;
      }, [value]);
      return null;
    }

    const root = createRoot(container);
    act(() => {
      root.render(
        <TooltipProvider {...props}>
          <Probe />
        </TooltipProvider>,
      );
    });
    return root;
  }

  test("exposes configured delay values to descendants", () => {
    const root = renderProvider({
      delayDuration: 500,
      skipDelayDuration: 200,
      hideGraceDuration: 80,
    });

    expect(coordinator?.config).toEqual({
      delayDuration: 500,
      skipDelayDuration: 200,
      hideGraceDuration: 80,
    });

    act(() => {
      root.unmount();
    });
  });

  test("activates skip-delay after a tooltip opens", () => {
    const root = renderProvider();
    const forceHide = vi.fn();

    coordinator?.registerTooltip("tooltip-a", forceHide);
    expect(coordinator?.isSkipDelayActive()).toBe(false);

    coordinator?.markOpened("tooltip-a");
    expect(coordinator?.isSkipDelayActive()).toBe(true);
    expect(forceHide).not.toHaveBeenCalled();

    coordinator?.markOpened("tooltip-b");
    expect(forceHide).toHaveBeenCalledTimes(1);

    act(() => {
      root.unmount();
    });
  });
});
