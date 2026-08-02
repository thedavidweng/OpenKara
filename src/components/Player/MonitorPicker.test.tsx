// @vitest-environment jsdom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { beforeEach, describe, expect, test, vi } from "vitest";
import { MonitorPicker } from "./MonitorPicker";

const {
  mockGetMonitors,
  mockOpenFullscreenPlayer,
  mockSyncAirPlayAudienceState,
} = vi.hoisted(() => ({
  mockGetMonitors: vi.fn(),
  mockOpenFullscreenPlayer: vi.fn(),
  mockSyncAirPlayAudienceState: vi.fn(),
}));

const mockTranslate = vi.hoisted(
  () => (key: string, options?: { index?: number }) =>
    ({
      "player.selectMonitor": "Select Monitor",
      "player.localDisplayOutput": "Local Display Output",
      "player.noDisplaysFound": "No displays found yet.",
      "player.monitor": `Monitor ${options?.index ?? ""}`.trim(),
      "player.unnamedMonitor": `Unnamed monitor ${options?.index ?? ""}`.trim(),
    })[key as keyof Record<string, string>] ?? key,
);

vi.mock("react-i18next", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react-i18next")>();
  return {
    ...actual,
    useTranslation: () => ({ t: mockTranslate }),
  };
});

vi.mock("@/lib/fullscreen-player", () => ({
  getMonitors: mockGetMonitors,
  openFullscreenPlayer: mockOpenFullscreenPlayer,
}));

vi.mock("@/lib/tauri", () => ({
  syncAirPlayAudienceState: mockSyncAirPlayAudienceState,
}));

interface MockMonitor {
  name: string | null;
  size: { width: number; height: number };
  position: { x: number; y: number };
}

function buildMonitor(
  name: string,
  index: number,
  overrides: Partial<MockMonitor> = {},
): MockMonitor {
  return {
    name,
    size: { width: 1920, height: 1080 },
    position: { x: 1920 * index, y: 0 },
    ...overrides,
  };
}

async function flushEffects() {
  await act(async () => {
    await Promise.resolve();
  });
}

describe("MonitorPicker", () => {
  beforeEach(() => {
    mockGetMonitors.mockReset();
    mockOpenFullscreenPlayer.mockReset();
    mockSyncAirPlayAudienceState.mockReset();
    mockGetMonitors.mockResolvedValue([buildMonitor("Studio Display", 0)]);
    mockSyncAirPlayAudienceState.mockResolvedValue(undefined);

    (
      globalThis as typeof globalThis & {
        IS_REACT_ACT_ENVIRONMENT?: boolean;
      }
    ).IS_REACT_ACT_ENVIRONMENT = true;

    Object.defineProperty(HTMLElement.prototype, "getBoundingClientRect", {
      configurable: true,
      value: () => ({
        width: 120,
        height: 32,
        top: 24,
        right: 144,
        bottom: 56,
        left: 24,
        x: 24,
        y: 24,
        toJSON: () => ({}),
      }),
    });
  });

  test("renders only the local display section", async () => {
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);
    const anchor = document.createElement("button");
    document.body.appendChild(anchor);

    await act(async () => {
      root.render(
        <MonitorPicker onClose={() => {}} anchorRef={{ current: anchor }} />,
      );
    });
    await flushEffects();

    expect(document.body.textContent).toContain("Local Display Output");
    expect(document.body.textContent).not.toContain("AirPlay Output");
    expect(document.body.textContent).not.toContain(
      "Choose an AirPlay device from the native system control below.",
    );

    await act(async () => {
      root.unmount();
    });
    anchor.remove();
    container.remove();
  });

  test("shows the empty-state copy when no displays are available", async () => {
    mockGetMonitors.mockResolvedValue([]);
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);
    const anchor = document.createElement("button");
    document.body.appendChild(anchor);

    await act(async () => {
      root.render(
        <MonitorPicker onClose={() => {}} anchorRef={{ current: anchor }} />,
      );
    });
    await flushEffects();

    expect(document.body.textContent).toContain("No displays found yet.");

    await act(async () => {
      root.unmount();
    });
    anchor.remove();
    container.remove();
  });

  test("uses a distinct translated fallback name for unnamed displays", async () => {
    mockGetMonitors.mockResolvedValue([buildMonitor("", 0, { name: null })]);
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);
    const anchor = document.createElement("button");
    document.body.appendChild(anchor);

    await act(async () => {
      root.render(
        <MonitorPicker onClose={() => {}} anchorRef={{ current: anchor }} />,
      );
    });
    await flushEffects();

    const option = document.querySelector('[role="option"]');
    expect(option?.textContent).toContain("Unnamed monitor 1");
    expect(option?.textContent).toContain("Monitor 1");

    await act(async () => root.unmount());
    anchor.remove();
    container.remove();
  });

  test("stops AirPlay output before opening a local audience display", async () => {
    const onClose = vi.fn();
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);
    const anchor = document.createElement("button");
    document.body.appendChild(anchor);

    await act(async () => {
      root.render(
        <MonitorPicker onClose={onClose} anchorRef={{ current: anchor }} />,
      );
    });
    await flushEffects();

    const monitorButton = [...document.body.querySelectorAll("button")].find(
      (button) =>
        button.textContent?.includes("Monitor 1") &&
        button.textContent?.includes("1920x1080"),
    );
    expect(monitorButton).toBeTruthy();

    await act(async () => {
      monitorButton?.dispatchEvent(
        new MouseEvent("click", { bubbles: true, cancelable: true }),
      );
    });
    await flushEffects();

    expect(mockSyncAirPlayAudienceState).toHaveBeenCalledWith(
      expect.objectContaining({ mode: "idle" }),
    );
    expect(mockOpenFullscreenPlayer).toHaveBeenCalledWith(0);
    expect(onClose).toHaveBeenCalledOnce();

    await act(async () => {
      root.unmount();
    });
    anchor.remove();
    container.remove();
  });

  test("renders role=listbox and role=option with aria-selected", async () => {
    mockGetMonitors.mockResolvedValue([
      buildMonitor("Display A", 0),
      buildMonitor("Display B", 1),
    ]);
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);
    const anchor = document.createElement("button");
    document.body.appendChild(anchor);

    await act(async () => {
      root.render(
        <MonitorPicker onClose={() => {}} anchorRef={{ current: anchor }} />,
      );
    });
    await flushEffects();

    const listbox = document.querySelector('[role="listbox"]');
    expect(listbox).not.toBeNull();

    const options = document.querySelectorAll('[role="option"]');
    expect(options).toHaveLength(2);
    expect(options[0].getAttribute("aria-selected")).toBe("true");
    expect(options[1].getAttribute("aria-selected")).toBe("false");

    await act(async () => root.unmount());
    anchor.remove();
    container.remove();
  });

  test("ArrowDown moves focus to the next monitor option", async () => {
    mockGetMonitors.mockResolvedValue([
      buildMonitor("Display A", 0),
      buildMonitor("Display B", 1),
      buildMonitor("Display C", 2),
    ]);
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);
    const anchor = document.createElement("button");
    document.body.appendChild(anchor);

    await act(async () => {
      root.render(
        <MonitorPicker onClose={() => {}} anchorRef={{ current: anchor }} />,
      );
    });
    await flushEffects();

    const listbox = document.querySelector('[role="listbox"]') as HTMLElement;
    const options = document.querySelectorAll('[role="option"]');

    expect(document.activeElement).toBe(options[0]);

    await act(async () => {
      listbox.dispatchEvent(
        new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }),
      );
    });
    expect(document.activeElement).toBe(options[1]);

    await act(async () => {
      listbox.dispatchEvent(
        new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }),
      );
    });
    expect(document.activeElement).toBe(options[2]);

    await act(async () => {
      listbox.dispatchEvent(
        new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }),
      );
    });
    expect(document.activeElement).toBe(options[0]);

    await act(async () => root.unmount());
    anchor.remove();
    container.remove();
  });

  test("ArrowUp moves focus to the previous monitor option", async () => {
    mockGetMonitors.mockResolvedValue([
      buildMonitor("Display A", 0),
      buildMonitor("Display B", 1),
    ]);
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);
    const anchor = document.createElement("button");
    document.body.appendChild(anchor);

    await act(async () => {
      root.render(
        <MonitorPicker onClose={() => {}} anchorRef={{ current: anchor }} />,
      );
    });
    await flushEffects();

    const listbox = document.querySelector('[role="listbox"]') as HTMLElement;
    const options = document.querySelectorAll('[role="option"]');

    await act(async () => {
      listbox.dispatchEvent(
        new KeyboardEvent("keydown", { key: "ArrowUp", bubbles: true }),
      );
    });
    expect(document.activeElement).toBe(options[1]);

    await act(async () => root.unmount());
    anchor.remove();
    container.remove();
  });

  test("Home and End move focus to first and last option", async () => {
    mockGetMonitors.mockResolvedValue([
      buildMonitor("Display A", 0),
      buildMonitor("Display B", 1),
      buildMonitor("Display C", 2),
    ]);
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);
    const anchor = document.createElement("button");
    document.body.appendChild(anchor);

    await act(async () => {
      root.render(
        <MonitorPicker onClose={() => {}} anchorRef={{ current: anchor }} />,
      );
    });
    await flushEffects();

    const listbox = document.querySelector('[role="listbox"]') as HTMLElement;
    const options = document.querySelectorAll('[role="option"]');

    await act(async () => {
      listbox.dispatchEvent(
        new KeyboardEvent("keydown", { key: "End", bubbles: true }),
      );
    });
    expect(document.activeElement).toBe(options[2]);

    await act(async () => {
      listbox.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Home", bubbles: true }),
      );
    });
    expect(document.activeElement).toBe(options[0]);

    await act(async () => root.unmount());
    anchor.remove();
    container.remove();
  });

  test("Escape closes the picker and returns focus to the anchor", async () => {
    mockGetMonitors.mockResolvedValue([buildMonitor("Display A", 0)]);
    const onClose = vi.fn();
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);
    const anchor = document.createElement("button");
    document.body.appendChild(anchor);

    await act(async () => {
      root.render(
        <MonitorPicker onClose={onClose} anchorRef={{ current: anchor }} />,
      );
    });
    await flushEffects();

    const listbox = document.querySelector('[role="listbox"]') as HTMLElement;

    await act(async () => {
      listbox.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
      );
    });

    expect(onClose).toHaveBeenCalled();
    expect(document.activeElement).toBe(anchor);

    await act(async () => root.unmount());
    anchor.remove();
    container.remove();
  });

  test("Enter selects the focused monitor and closes the picker", async () => {
    mockGetMonitors.mockResolvedValue([
      buildMonitor("Display A", 0),
      buildMonitor("Display B", 1),
    ]);
    const onClose = vi.fn();
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);
    const anchor = document.createElement("button");
    document.body.appendChild(anchor);

    await act(async () => {
      root.render(
        <MonitorPicker onClose={onClose} anchorRef={{ current: anchor }} />,
      );
    });
    await flushEffects();

    const listbox = document.querySelector('[role="listbox"]') as HTMLElement;

    await act(async () => {
      listbox.dispatchEvent(
        new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }),
      );
    });
    await act(async () => {
      listbox.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
      );
    });
    await flushEffects();

    expect(mockOpenFullscreenPlayer).toHaveBeenCalledWith(1);
    expect(onClose).toHaveBeenCalled();

    await act(async () => root.unmount());
    anchor.remove();
    container.remove();
  });

  test("mouse enter on an option sets focus index", async () => {
    mockGetMonitors.mockResolvedValue([
      buildMonitor("Display A", 0),
      buildMonitor("Display B", 1),
    ]);
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);
    const anchor = document.createElement("button");
    document.body.appendChild(anchor);

    await act(async () => {
      root.render(
        <MonitorPicker onClose={() => {}} anchorRef={{ current: anchor }} />,
      );
    });
    await flushEffects();

    const options = document.querySelectorAll('[role="option"]');
    const secondOption = options[1] as HTMLElement;

    await act(async () => {
      secondOption.dispatchEvent(
        new MouseEvent("mouseover", {
          bubbles: true,
          relatedTarget: document.body,
        }),
      );
    });

    expect(secondOption.tabIndex).toBe(0);
    expect(secondOption.getAttribute("aria-selected")).toBe("true");

    await act(async () => root.unmount());
    anchor.remove();
    container.remove();
  });

  test("document-level Escape closes the picker and returns focus to the anchor", async () => {
    mockGetMonitors.mockResolvedValue([buildMonitor("Display A", 0)]);
    const onClose = vi.fn();
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);
    const anchor = document.createElement("button");
    document.body.appendChild(anchor);

    await act(async () => {
      root.render(
        <MonitorPicker onClose={onClose} anchorRef={{ current: anchor }} />,
      );
    });
    await flushEffects();

    await act(async () => {
      document.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
      );
    });

    expect(onClose).toHaveBeenCalled();
    expect(document.activeElement).toBe(anchor);

    await act(async () => root.unmount());
    anchor.remove();
    container.remove();
  });
});
