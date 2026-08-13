// @vitest-environment jsdom

import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, test, vi } from "vitest";
import { cleanup, fireEvent, render } from "@testing-library/react";
import type { ActiveTask } from "@/lib/task-progress";
import { GlobalProgressBar, TaskProgressBar } from "./GlobalProgressBar";

const { activeTasks } = vi.hoisted(() => ({
  activeTasks: { current: [] as ActiveTask[] },
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock("@/hooks/use-active-tasks", () => ({
  useActiveTasks: () => activeTasks.current,
}));

describe("GlobalProgressBar", () => {
  afterEach(() => {
    cleanup();
    activeTasks.current = [];
    vi.clearAllMocks();
  });

  test("renders nothing while no task is active", () => {
    expect(renderToStaticMarkup(<GlobalProgressBar />)).toBe("");
  });

  test("renders one bar per active task with its label, detail and fill", () => {
    activeTasks.current = [
      { key: "sep-song-a", label: "Separating", detail: "Song A", percent: 42 },
      {
        key: "runtime-download",
        label: "Downloading runtime",
        percent: 0,
        indeterminate: true,
      },
    ];

    const markup = renderToStaticMarkup(<GlobalProgressBar />);

    expect(markup).toContain("Separating");
    expect(markup).toContain("Song A");
    expect(markup).toContain("width:42%");
    expect(markup).toContain("Downloading runtime");
    expect(markup).toContain("model-indeterminate-bar");
  });

  test("invokes the task cancel affordance", () => {
    const onCancel = vi.fn();
    activeTasks.current = [
      { key: "batch-separation", label: "Separating", percent: 10, onCancel },
    ];

    const { getByRole } = render(<GlobalProgressBar />);
    fireEvent.click(getByRole("button"));

    expect(onCancel).toHaveBeenCalledTimes(1);
  });

  test("TaskProgressBar compact mode clamps percent and supports indeterminate fill", () => {
    const clamped = renderToStaticMarkup(
      <TaskProgressBar compact label="" ariaLabel="batch-song" percent={140} />,
    );
    expect(clamped).toContain('role="progressbar"');
    expect(clamped).toContain('aria-label="batch-song"');
    expect(clamped).toContain("width:100%");
    expect(clamped).toContain("h-1 w-full");

    const below = renderToStaticMarkup(
      <TaskProgressBar compact label="x" percent={-10} />,
    );
    expect(below).toContain("width:0%");
    expect(below).toContain('aria-label="x"');

    const indeterminate = renderToStaticMarkup(
      <TaskProgressBar
        compact
        label=""
        percent={0}
        indeterminate
        ariaLabel="unknown-total"
      />,
    );
    expect(indeterminate).toContain("model-indeterminate-bar");
    expect(indeterminate).toContain('aria-label="unknown-total"');
  });

  test("TaskProgressBar exposes non-compact progress and a named cancel control", () => {
    const markup = renderToStaticMarkup(
      <TaskProgressBar
        label="Separating song"
        percent={42}
        onCancel={vi.fn()}
      />,
    );

    expect(markup).toContain('role="progressbar"');
    expect(markup).toContain('aria-label="Separating song"');
    expect(markup).toContain('aria-valuenow="42"');
    expect(markup).toContain('aria-label="common.cancel"');
  });
});
