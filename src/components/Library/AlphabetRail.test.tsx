// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { type AlphabetBucket } from "@/lib/alphabet-index";
import { AlphabetRail } from "./AlphabetRail";

const { mockUseTranslation } = vi.hoisted(() => ({
  mockUseTranslation: vi.fn(() => ({
    t: (key: string, opts?: { letter?: string }) =>
      key === "sidebar.alphabetRail.jumpTo"
        ? `Jump to ${opts?.letter ?? ""}`
        : key === "sidebar.alphabetRail.other"
          ? "Other characters"
          : key,
  })),
}));

vi.mock("react-i18next", () => ({
  useTranslation: mockUseTranslation,
}));

function makeIndex(
  entries: [AlphabetBucket, number][],
): ReadonlyMap<AlphabetBucket, number> {
  return new Map(entries);
}

describe("AlphabetRail", () => {
  beforeEach(() => {
    cleanup();
    mockUseTranslation.mockReset();
    mockUseTranslation.mockReturnValue({
      t: (key: string, opts?: { letter?: string }) =>
        key === "sidebar.alphabetRail.jumpTo"
          ? `Jump to ${opts?.letter ?? ""}`
          : key === "sidebar.alphabetRail.other"
            ? "Other characters"
            : key,
    });
  });

  afterEach(() => {
    cleanup();
  });

  test("renders 27 letter buttons", () => {
    const index = makeIndex([["A", 0]]);
    render(<AlphabetRail indexByBucket={index} onNavigate={vi.fn()} />);
    const buttons = screen.getAllByRole("button");
    expect(buttons).toHaveLength(27);
    expect(buttons[0].textContent).toBe("A");
    expect(buttons[26].textContent).toBe("#");
  });

  test("click on mapped bucket calls onNavigate with correct index", () => {
    const onNavigate = vi.fn();
    const index = makeIndex([
      ["A", 0],
      ["B", 5],
    ]);
    render(<AlphabetRail indexByBucket={index} onNavigate={onNavigate} />);
    const buttonB = screen.getAllByRole("button")[1];
    fireEvent.click(buttonB);
    expect(onNavigate).toHaveBeenCalledWith(5, "B");
  });

  test("click on missing bucket resolves to nearest mapped", () => {
    const onNavigate = vi.fn();
    const index = makeIndex([
      ["A", 0],
      ["Z", 25],
    ]);
    render(<AlphabetRail indexByBucket={index} onNavigate={onNavigate} />);
    const buttonC = screen.getAllByRole("button")[2];
    fireEvent.click(buttonC);
    // C is distance 2 from A, distance 23 from Z → resolves to A
    expect(onNavigate).toHaveBeenCalledWith(0, "A");
  });

  test("does not call onNavigate when index map is empty", () => {
    const onNavigate = vi.fn();
    const index = makeIndex([]);
    render(<AlphabetRail indexByBucket={index} onNavigate={onNavigate} />);
    const buttonA = screen.getAllByRole("button")[0];
    fireEvent.click(buttonA);
    expect(onNavigate).not.toHaveBeenCalled();
  });

  test("dedupes consecutive navigations to the same bucket", () => {
    const onNavigate = vi.fn();
    const index = makeIndex([["A", 0]]);
    render(<AlphabetRail indexByBucket={index} onNavigate={onNavigate} />);
    const buttonA = screen.getAllByRole("button")[0];
    fireEvent.click(buttonA);
    fireEvent.click(buttonA);
    // Second click to the same resolved bucket should be deduped
    expect(onNavigate).toHaveBeenCalledTimes(1);
  });

  test("roving tabindex: only one button has tabindex 0", () => {
    const index = makeIndex([
      ["A", 0],
      ["B", 5],
    ]);
    render(<AlphabetRail indexByBucket={index} onNavigate={vi.fn()} />);
    const buttons = screen.getAllByRole("button");
    const tabbable = buttons.filter((b) => b.getAttribute("tabindex") === "0");
    expect(tabbable).toHaveLength(1);
  });

  test("ArrowDown moves roving tabindex to next bucket", () => {
    const index = makeIndex([["A", 0]]);
    render(<AlphabetRail indexByBucket={index} onNavigate={vi.fn()} />);
    const container = screen.getByRole("navigation");
    fireEvent.keyDown(container, { key: "ArrowDown" });
    const buttons = screen.getAllByRole("button");
    expect(buttons[1].getAttribute("tabindex")).toBe("0");
  });

  test("Home moves roving to A", () => {
    const index = makeIndex([["M", 13]]);
    render(<AlphabetRail indexByBucket={index} onNavigate={vi.fn()} />);
    const container = screen.getByRole("navigation");
    fireEvent.keyDown(container, { key: "Home" });
    const buttons = screen.getAllByRole("button");
    expect(buttons[0].getAttribute("tabindex")).toBe("0");
  });

  test("End moves roving to #", () => {
    const index = makeIndex([["A", 0]]);
    render(<AlphabetRail indexByBucket={index} onNavigate={vi.fn()} />);
    const container = screen.getByRole("navigation");
    fireEvent.keyDown(container, { key: "End" });
    const buttons = screen.getAllByRole("button");
    expect(buttons[26].getAttribute("tabindex")).toBe("0");
  });

  test("Enter activates the roving bucket", () => {
    const onNavigate = vi.fn();
    const index = makeIndex([["A", 0]]);
    render(<AlphabetRail indexByBucket={index} onNavigate={onNavigate} />);
    const container = screen.getByRole("navigation");
    // A is the initial roving bucket
    fireEvent.keyDown(container, { key: "Enter" });
    expect(onNavigate).toHaveBeenCalledWith(0, "A");
  });

  test("typeahead focuses and activates the typed letter", () => {
    const onNavigate = vi.fn();
    const index = makeIndex([["M", 13]]);
    render(<AlphabetRail indexByBucket={index} onNavigate={onNavigate} />);
    const container = screen.getByRole("navigation");
    fireEvent.keyDown(container, { key: "m" });
    expect(onNavigate).toHaveBeenCalledWith(13, "M");
  });

  test("typeahead allows repeat navigation for the same letter", () => {
    const onNavigate = vi.fn();
    const index = makeIndex([["A", 0]]);
    render(<AlphabetRail indexByBucket={index} onNavigate={onNavigate} />);
    const container = screen.getByRole("navigation");
    fireEvent.keyDown(container, { key: "a" });
    expect(onNavigate).toHaveBeenCalledTimes(1);
    // Second press of the same letter should navigate again, not be deduped
    fireEvent.keyDown(container, { key: "a" });
    expect(onNavigate).toHaveBeenCalledTimes(2);
  });

  test("localized labels are applied", () => {
    const index = makeIndex([["A", 0]]);
    render(<AlphabetRail indexByBucket={index} onNavigate={vi.fn()} />);
    const buttons = screen.getAllByRole("button");
    expect(buttons[0].getAttribute("aria-label")).toBe("Jump to A");
    expect(buttons[26].getAttribute("aria-label")).toBe("Other characters");
  });

  test("aria-current is set on the active bucket after navigation", () => {
    const index = makeIndex([["A", 0]]);
    render(<AlphabetRail indexByBucket={index} onNavigate={vi.fn()} />);
    const buttonA = screen.getAllByRole("button")[0];
    fireEvent.click(buttonA);
    expect(buttonA.getAttribute("aria-current")).toBe("true");
  });

  test("index map change resets transient state", () => {
    const onNavigate = vi.fn();
    const index1 = makeIndex([
      ["A", 0],
      ["B", 5],
    ]);
    const { rerender } = render(
      <AlphabetRail indexByBucket={index1} onNavigate={onNavigate} />,
    );
    // Activate B
    const buttons = screen.getAllByRole("button");
    fireEvent.click(buttons[1]);
    expect(buttons[1].getAttribute("aria-current")).toBe("true");

    // Change index map
    const index2 = makeIndex([["C", 10]]);
    rerender(<AlphabetRail indexByBucket={index2} onNavigate={onNavigate} />);
    // No aria-current should be set after reset
    const activeButtons = screen
      .getAllByRole("button")
      .filter((b) => b.getAttribute("aria-current") === "true");
    expect(activeButtons).toHaveLength(0);
  });

  // ─── keyboard: ArrowUp / ArrowLeft ─────────────────────────

  test("ArrowUp moves roving tabindex to the previous bucket", () => {
    const index = makeIndex([["B", 5]]);
    render(<AlphabetRail indexByBucket={index} onNavigate={vi.fn()} />);
    const container = screen.getByRole("navigation");
    // B is the first mapped bucket → initial roving. ArrowUp → A.
    fireEvent.keyDown(container, { key: "ArrowUp" });
    const buttons = screen.getAllByRole("button");
    expect(buttons[0].getAttribute("tabindex")).toBe("0");
  });

  test("ArrowLeft moves roving tabindex to the previous bucket", () => {
    const index = makeIndex([["B", 5]]);
    render(<AlphabetRail indexByBucket={index} onNavigate={vi.fn()} />);
    const container = screen.getByRole("navigation");
    fireEvent.keyDown(container, { key: "ArrowLeft" });
    const buttons = screen.getAllByRole("button");
    expect(buttons[0].getAttribute("tabindex")).toBe("0");
  });

  // ─── pointer-based navigation ──────────────────────────────

  // Mock geometry so each of the 27 buckets spans 10px (height 270, top 0).
  function mockContainerGeometry(): DOMRect {
    return {
      x: 0,
      y: 0,
      top: 0,
      left: 0,
      right: 22,
      bottom: 270,
      width: 22,
      height: 270,
      toJSON: () => ({}),
    };
  }

  function setupPointerContainer() {
    const container = screen.getByRole("navigation");
    vi.spyOn(container, "getBoundingClientRect").mockReturnValue(
      mockContainerGeometry(),
    );
    container.setPointerCapture = vi.fn();
    container.releasePointerCapture = vi.fn();
    return container;
  }

  test("pointer down navigates to the bucket at the pointer position", () => {
    const onNavigate = vi.fn();
    const index = makeIndex([
      ["A", 0],
      ["B", 5],
    ]);
    render(<AlphabetRail indexByBucket={index} onNavigate={onNavigate} />);
    const container = setupPointerContainer();
    // clientY=15 → fraction 15/270 ≈ 0.056 → raw floor(1.5) = 1 → bucket B
    fireEvent.pointerDown(container, { button: 0, pointerId: 1, clientY: 15 });
    expect(onNavigate).toHaveBeenCalledWith(5, "B");
    expect(container.setPointerCapture).toHaveBeenCalledWith(1);
  });

  test("ignores pointer down for non-primary buttons", () => {
    const onNavigate = vi.fn();
    const index = makeIndex([["A", 0]]);
    render(<AlphabetRail indexByBucket={index} onNavigate={onNavigate} />);
    const container = setupPointerContainer();
    fireEvent.pointerDown(container, { button: 2, pointerId: 1, clientY: 0 });
    expect(onNavigate).not.toHaveBeenCalled();
    expect(container.setPointerCapture).not.toHaveBeenCalled();
  });

  test("falls back to # bucket when geometry is unavailable", () => {
    const onNavigate = vi.fn();
    const index = makeIndex([["#", 10]]);
    render(<AlphabetRail indexByBucket={index} onNavigate={onNavigate} />);
    const container = screen.getByRole("navigation");
    vi.spyOn(container, "getBoundingClientRect").mockReturnValue(
      undefined as unknown as DOMRect,
    );
    container.setPointerCapture = vi.fn();
    fireEvent.pointerDown(container, { button: 0, pointerId: 1, clientY: 999 });
    expect(onNavigate).toHaveBeenCalledWith(10, "#");
  });

  test("treats zero-height geometry as the first bucket", () => {
    const onNavigate = vi.fn();
    const index = makeIndex([
      ["A", 0],
      ["Z", 25],
    ]);
    render(<AlphabetRail indexByBucket={index} onNavigate={onNavigate} />);
    const container = screen.getByRole("navigation");
    vi.spyOn(container, "getBoundingClientRect").mockReturnValue({
      top: 0,
      bottom: 0,
      left: 0,
      right: 12,
      width: 12,
      height: 0,
      x: 0,
      y: 0,
      toJSON: () => ({}),
    } as DOMRect);
    container.setPointerCapture = vi.fn();
    fireEvent.pointerDown(container, {
      button: 0,
      pointerId: 1,
      clientY: 100,
    });
    // height <= 0 forces fraction 0 → bucket A
    expect(onNavigate).toHaveBeenCalledWith(0, "A");
  });

  test("pointer move during drag navigates to the bucket under the pointer", () => {
    const onNavigate = vi.fn();
    const index = makeIndex([
      ["A", 0],
      ["M", 13],
    ]);
    render(<AlphabetRail indexByBucket={index} onNavigate={onNavigate} />);
    const container = setupPointerContainer();
    // Start drag on A (clientY=0 → bucket A).
    fireEvent.pointerDown(container, { button: 0, pointerId: 1, clientY: 0 });
    expect(onNavigate).toHaveBeenCalledWith(0, "A");
    // Drag to M (clientY=125 → fraction ≈ 0.463 → raw floor(12.5) = 12 → M).
    fireEvent.pointerMove(container, { pointerId: 1, clientY: 125 });
    expect(onNavigate).toHaveBeenCalledWith(13, "M");
  });

  test("pointer move is ignored without an active pointer capture", () => {
    const onNavigate = vi.fn();
    const index = makeIndex([["A", 0]]);
    render(<AlphabetRail indexByBucket={index} onNavigate={onNavigate} />);
    const container = setupPointerContainer();
    // No preceding pointerDown → activePointerIdRef is null → ignored.
    fireEvent.pointerMove(container, { pointerId: 1, clientY: 125 });
    expect(onNavigate).not.toHaveBeenCalled();
  });

  test("pointer up after a drag releases capture and clears the active bucket", () => {
    const onNavigate = vi.fn();
    const index = makeIndex([
      ["A", 0],
      ["B", 5],
    ]);
    render(<AlphabetRail indexByBucket={index} onNavigate={onNavigate} />);
    const container = setupPointerContainer();
    // Start drag on A (clientY=0 → bucket A).
    fireEvent.pointerDown(container, { button: 0, pointerId: 1, clientY: 0 });
    // Drag to B (clientY=15 → bucket B).
    fireEvent.pointerMove(container, { pointerId: 1, clientY: 15 });
    fireEvent.pointerUp(container, { pointerId: 1 });
    expect(container.releasePointerCapture).toHaveBeenCalledWith(1);
    const activeButtons = screen
      .getAllByRole("button")
      .filter((b) => b.getAttribute("aria-current") === "true");
    expect(activeButtons).toHaveLength(0);
  });

  test("simple tap (pointerdown + pointerup + click) persists the active bucket", () => {
    const onNavigate = vi.fn();
    const index = makeIndex([["A", 0]]);
    render(<AlphabetRail indexByBucket={index} onNavigate={onNavigate} />);
    const container = setupPointerContainer();
    // A simple tap: pointerdown, pointerup (no move), then synthetic click.
    fireEvent.pointerDown(container, { button: 0, pointerId: 1, clientY: 0 });
    fireEvent.pointerUp(container, { pointerId: 1 });
    const buttonA = screen.getAllByRole("button")[0];
    fireEvent.click(buttonA);
    // The current-section marker must persist after a simple tap.
    expect(buttonA.getAttribute("aria-current")).toBe("true");
  });

  test("suppresses synthetic click after pointer-based navigation", () => {
    const onNavigate = vi.fn();
    const index = makeIndex([["A", 0]]);
    render(<AlphabetRail indexByBucket={index} onNavigate={onNavigate} />);
    const container = setupPointerContainer();
    fireEvent.pointerDown(container, { button: 0, pointerId: 1, clientY: 0 });
    expect(onNavigate).toHaveBeenCalledTimes(1);
    // The synthetic click that follows a pointer drag must not double-navigate.
    const buttonA = screen.getAllByRole("button")[0];
    fireEvent.click(buttonA);
    expect(onNavigate).toHaveBeenCalledTimes(1);
  });

  test("releasePointerCapture throw is swallowed on pointer up", () => {
    const onNavigate = vi.fn();
    const index = makeIndex([["A", 0]]);
    render(<AlphabetRail indexByBucket={index} onNavigate={onNavigate} />);
    const container = setupPointerContainer();
    container.releasePointerCapture = vi.fn(() => {
      throw new Error("already released");
    });
    fireEvent.pointerDown(container, { button: 0, pointerId: 7, clientY: 0 });
    expect(() =>
      fireEvent.pointerUp(container, { button: 0, pointerId: 7, clientY: 0 }),
    ).not.toThrow();
  });

  test("pointer move with mismatched pointer id is ignored", () => {
    const onNavigate = vi.fn();
    const index = makeIndex([
      ["A", 0],
      ["B", 1],
    ]);
    render(<AlphabetRail indexByBucket={index} onNavigate={onNavigate} />);
    const container = setupPointerContainer();
    fireEvent.pointerDown(container, { button: 0, pointerId: 1, clientY: 0 });
    onNavigate.mockClear();
    fireEvent.pointerMove(container, {
      button: 0,
      pointerId: 99,
      clientY: 100,
    });
    expect(onNavigate).not.toHaveBeenCalled();
  });

  test("pointer up with mismatched pointer id is ignored", () => {
    const onNavigate = vi.fn();
    const index = makeIndex([["A", 0]]);
    render(<AlphabetRail indexByBucket={index} onNavigate={onNavigate} />);
    const container = setupPointerContainer();
    fireEvent.pointerDown(container, { button: 0, pointerId: 1, clientY: 0 });
    // Wrong pointer id should not clear active state / release capture.
    fireEvent.pointerUp(container, { button: 0, pointerId: 99, clientY: 0 });
    expect(container.releasePointerCapture).not.toHaveBeenCalled();
    // Correct id still releases.
    fireEvent.pointerUp(container, { button: 0, pointerId: 1, clientY: 0 });
    expect(container.releasePointerCapture).toHaveBeenCalledWith(1);
  });

  test("empty index map starts with null roving and keyboard uses pos 0", () => {
    const onNavigate = vi.fn();
    // Empty map → useEffect sets rovingBucket to null → keydown takes the :0 branch.
    const index = new Map();
    render(<AlphabetRail indexByBucket={index} onNavigate={onNavigate} />);
    const container = screen.getByRole("navigation");
    fireEvent.keyDown(container, { key: "ArrowDown" });
    // With null roving, currentPos falls back to 0 (bucket A). ArrowDown → B.
    // But B has no mapping so navigation may no-op; the :0 branch is still executed.
    const buttons = screen.getAllByRole("button");
    // After ArrowDown, roving should move to B (index 1) even without mapping.
    expect(buttons[1].getAttribute("tabindex")).toBe("0");
  });
});
