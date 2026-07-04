// @vitest-environment jsdom

import userEvent from "@testing-library/user-event";
import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { Tooltip, TooltipProvider } from "./Tooltip";

async function flushEffects() {
  await act(async () => {
    await Promise.resolve();
  });
}

describe("Tooltip", () => {
  let container: HTMLDivElement;
  let root: Root;
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    (
      globalThis as typeof globalThis & {
        IS_REACT_ACT_ENVIRONMENT?: boolean;
      }
    ).IS_REACT_ACT_ENVIRONMENT = true;

    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
    user = userEvent.setup();
  });

  afterEach(() => {
    act(() => {
      root.unmount();
    });
    container.remove();
    document.body
      .querySelectorAll('[role="tooltip"]')
      .forEach((node) => node.remove());
    vi.restoreAllMocks();
  });

  function renderTooltip(
    ui: ReactNode,
    options?: {
      withProvider?: boolean;
      providerProps?: Omit<
        React.ComponentProps<typeof TooltipProvider>,
        "children"
      >;
    },
  ) {
    const withProvider = options?.withProvider ?? true;
    act(() => {
      root.render(
        withProvider ? (
          <TooltipProvider {...options?.providerProps}>{ui}</TooltipProvider>
        ) : (
          ui
        ),
      );
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

  test("works with the fallback coordinator when no provider is mounted", async () => {
    renderTooltip(
      <Tooltip label="Offline tooltip">
        <button type="button">Action</button>
      </Tooltip>,
      { withProvider: false },
    );

    act(() => {
      getTriggerWrappers()[0]
        ?.querySelector("button")
        ?.dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
    });
    await flushEffects();

    expect(getTooltip()?.textContent).toContain("Offline tooltip");
  });

  test("shows after hover delay and hides after pointer leave", async () => {
    renderTooltip(
      <Tooltip label="Import files" delayDuration={20}>
        <button type="button">Import</button>
      </Tooltip>,
      { providerProps: { hideGraceDuration: 20 } },
    );

    const wrapper = getTriggerWrappers()[0] as HTMLElement;
    await user.hover(wrapper);
    expect(getTooltip()).toBeNull();

    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 25));
    });
    expect(getTooltip()?.textContent).toContain("Import files");

    await user.unhover(wrapper);
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 25));
    });
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

  test("merges aria-describedby with an existing child value", async () => {
    renderTooltip(
      <Tooltip label="Romanize">
        <button type="button" aria-describedby="existing-help">
          Romanize
        </button>
      </Tooltip>,
    );

    act(() => {
      getTriggerWrappers()[0]
        ?.querySelector("button")
        ?.dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
    });
    await flushEffects();

    const describedBy = getTriggerWrappers()[0]
      ?.querySelector("button")
      ?.getAttribute("aria-describedby");
    expect(describedBy).toContain("existing-help");
    expect(describedBy?.split(/\s+/).length).toBeGreaterThan(1);
  });

  test("leaves non-element children untouched when open", async () => {
    renderTooltip(<Tooltip label="Label">Plain text child</Tooltip>);

    act(() => {
      getTriggerWrappers()[0]?.dispatchEvent(
        new FocusEvent("focusin", { bubbles: true }),
      );
    });
    await flushEffects();

    expect(container.textContent).toContain("Plain text child");
    expect(getTooltip()?.textContent).toContain("Label");
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

  test("keeps the tooltip open when focus moves within the trigger", async () => {
    renderTooltip(
      <Tooltip label="Stem controls">
        <span>
          <button type="button">First</button>
          <button type="button">Second</button>
        </span>
      </Tooltip>,
    );

    const [firstButton, secondButton] = container.querySelectorAll("button");

    act(() => {
      firstButton.dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
    });
    await flushEffects();
    expect(getTooltip()).not.toBeNull();

    act(() => {
      firstButton.dispatchEvent(
        new FocusEvent("focusout", {
          bubbles: true,
          relatedTarget: secondButton,
        }),
      );
      secondButton.dispatchEvent(
        new FocusEvent("focusin", {
          bubbles: true,
          relatedTarget: firstButton,
        }),
      );
    });
    await flushEffects();
    expect(getTooltip()).not.toBeNull();
  });

  test("force-hides the previous tooltip when another one opens", async () => {
    renderTooltip(
      <>
        <Tooltip label="First action">
          <button type="button">First</button>
        </Tooltip>
        <Tooltip label="Second action">
          <button type="button">Second</button>
        </Tooltip>
      </>,
    );

    const buttons = container.querySelectorAll("button");
    act(() => {
      buttons[0]?.dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
    });
    await flushEffects();
    expect(getTooltip()?.textContent).toContain("First action");

    act(() => {
      buttons[1]?.dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
    });
    await flushEffects();
    expect(getTooltip()?.textContent).toContain("Second action");
  });

  test("ignores duplicate show requests while already open", async () => {
    renderTooltip(
      <Tooltip label="Queue">
        <button type="button">Queue</button>
      </Tooltip>,
    );

    const button = getTriggerWrappers()[0]?.querySelector("button");
    act(() => {
      button?.dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
      button?.dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
    });
    await flushEffects();

    expect(getTooltip()?.textContent).toContain("Queue");
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
    };

    Object.defineProperty(HTMLElement.prototype, "getBoundingClientRect", {
      configurable: true,
      value: () => rect as DOMRect,
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

  test("does not error when leaving before a delayed hover tooltip opens", async () => {
    renderTooltip(
      <Tooltip label="Late tooltip" delayDuration={50}>
        <button type="button">Late</button>
      </Tooltip>,
      { providerProps: { hideGraceDuration: 20 } },
    );

    const wrapper = getTriggerWrappers()[0] as HTMLElement;
    await user.hover(wrapper);
    await user.unhover(wrapper);

    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 80));
    });

    expect(getTooltip()).toBeNull();
  });

  test("repositions the tooltip on resize and scroll", async () => {
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
    };

    Object.defineProperty(HTMLElement.prototype, "getBoundingClientRect", {
      configurable: true,
      value: () => rect as DOMRect,
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

    rect.left = 260;
    rect.x = 260;
    rect.right = 300;

    act(() => {
      window.dispatchEvent(new Event("resize"));
      window.dispatchEvent(new Event("scroll"));
    });
    await flushEffects();

    const tooltip = getTooltip() as HTMLDivElement | null;
    expect(tooltip?.style.left).toBe("220px");
  });
});
