// @vitest-environment jsdom

import { act, type ComponentProps, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test } from "vitest";
import { Tooltip, TooltipProvider } from "./Tooltip";

async function flushEffects() {
  await act(async () => {
    await Promise.resolve();
  });
}

describe("Tooltip", () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    (
      globalThis as typeof globalThis & {
        IS_REACT_ACT_ENVIRONMENT?: boolean;
      }
    ).IS_REACT_ACT_ENVIRONMENT = true;

    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
  });

  afterEach(() => {
    act(() => {
      root.unmount();
    });
    container.remove();
    document.body
      .querySelectorAll('[role="tooltip"]')
      .forEach((node) => node.remove());
  });

  function renderTooltip(
    ui: ReactNode,
    providerProps?: ComponentProps<typeof TooltipProvider>,
  ) {
    act(() => {
      root.render(<TooltipProvider {...providerProps}>{ui}</TooltipProvider>);
    });
  }

  function getTriggerWrappers() {
    return container.querySelectorAll("span.inline-flex");
  }

  function getTooltip() {
    return document.body.querySelector('[role="tooltip"]');
  }

  test("renders children without a tooltip when disabled", () => {
    renderTooltip(
      <Tooltip label="Play" disabled>
        <button type="button">Play</button>
      </Tooltip>,
    );

    expect(container.querySelector("span.inline-flex")).toBeNull();
    expect(container.querySelector("button")).not.toBeNull();
    expect(getTooltip()).toBeNull();
  });

  test("shows immediately on keyboard focus", async () => {
    renderTooltip(
      <Tooltip label="Settings" shortcut="⌘,">
        <button type="button">Settings</button>
      </Tooltip>,
    );

    act(() => {
      getTriggerWrappers()[0]
        ?.querySelector("button")
        ?.dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
    });
    await flushEffects();

    expect(getTooltip()?.textContent).toContain("Settings");
    expect(getTooltip()?.textContent).toContain("⌘,");
    expect(
      getTriggerWrappers()[0]
        ?.querySelector("button")
        ?.getAttribute("aria-describedby"),
    ).toBeTruthy();
  });

  test("hides on blur and Escape", async () => {
    renderTooltip(
      <Tooltip label="Queue">
        <button type="button">Queue</button>
      </Tooltip>,
    );

    const button = getTriggerWrappers()[0]?.querySelector("button");

    act(() => {
      button?.dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
    });
    await flushEffects();
    expect(getTooltip()).not.toBeNull();

    act(() => {
      document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    });
    await flushEffects();
    expect(getTooltip()).toBeNull();

    act(() => {
      button?.dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
    });
    await flushEffects();
    expect(getTooltip()).not.toBeNull();

    act(() => {
      button?.dispatchEvent(new FocusEvent("focusout", { bubbles: true }));
    });
    await flushEffects();
    expect(getTooltip()).toBeNull();
  });

  test("positions the tooltip from the trigger bounds", async () => {
    const rect = {
      top: 120,
      left: 200,
      width: 40,
      height: 32,
      bottom: 152,
      right: 240,
      x: 200,
      y: 120,
      toJSON: () => ({}),
    } as DOMRect;

    Object.defineProperty(HTMLElement.prototype, "getBoundingClientRect", {
      configurable: true,
      value: () => rect,
    });
    Object.defineProperty(HTMLElement.prototype, "offsetWidth", {
      configurable: true,
      get() {
        return 120;
      },
    });
    Object.defineProperty(HTMLElement.prototype, "offsetHeight", {
      configurable: true,
      get() {
        return 36;
      },
    });

    renderTooltip(
      <Tooltip label="Stem mix">
        <button type="button">Stems</button>
      </Tooltip>,
    );

    act(() => {
      getTriggerWrappers()[0]
        ?.querySelector("button")
        ?.dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
    });
    await flushEffects();

    const tooltip = getTooltip() as HTMLDivElement | null;
    expect(tooltip?.style.left).toBe("160px");
    expect(tooltip?.style.top).toBe("76px");
  });
});
