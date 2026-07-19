import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test, vi } from "vitest";
import { QueueButton } from "./QueueButton";

const { mockQueueState } = vi.hoisted(() => ({
  mockQueueState: {
    queue: [] as string[],
    togglePanel: vi.fn(),
    isOpen: false,
  },
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

vi.mock("@/stores/queue-store", () => ({
  useQueueStore: (selector: (state: typeof mockQueueState) => unknown) =>
    selector(mockQueueState),
}));

vi.mock("@/components/Overlay/Tooltip", () => ({
  Tooltip: ({ children }: { children: React.ReactNode }) => children,
}));

describe("QueueButton", () => {
  test("exposes the unified queue button chrome", () => {
    const markup = renderToStaticMarkup(<QueueButton />);

    expect(markup).toContain('data-queue-button-visual-variant="unified"');
    expect(markup).toContain('aria-label="queue.title"');
  });

  test("uses the shared playback-bar-action-button class and 18px icon", () => {
    const markup = renderToStaticMarkup(<QueueButton />);

    expect(markup).toContain("playback-bar-action-button");
    expect(markup).toContain('data-playback-action="queue"');
    expect(markup).toContain('aria-pressed="false"');
    // 18px icon via lucide size prop renders as width="18" height="18"
    expect(markup).toContain('width="18"');
    expect(markup).toContain('height="18"');
    // No conflicting geometry classes from the old layout
    expect(markup).not.toContain("min-h-11");
    expect(markup).not.toContain("min-w-11");
    expect(markup).not.toContain("p-2.5");
    expect(markup).not.toContain("rounded-[14px]");
  });

  test("reflects the open state via aria-pressed and data-active", () => {
    mockQueueState.isOpen = true;
    const markup = renderToStaticMarkup(<QueueButton />);

    expect(markup).toContain('aria-pressed="true"');
    expect(markup).toContain('data-active="true"');
    mockQueueState.isOpen = false;
  });

  test("renders the queue badge with on-accent color when queue is non-empty", () => {
    mockQueueState.queue = [
      { hash: "a" },
      { hash: "b" },
    ] as unknown as typeof mockQueueState.queue;
    const markup = renderToStaticMarkup(<QueueButton />);

    expect(markup).toContain("text-[var(--color-on-accent)]");
    expect(markup).toContain(">2<");
    mockQueueState.queue = [];
  });
});

test("shows queue count badge with on-accent token when queue has items", () => {
  mockQueueState.queue = ["song-1", "song-2"];
  mockQueueState.isOpen = false;
  const markup = renderToStaticMarkup(<QueueButton />);
  expect(markup).toContain("text-[var(--color-on-accent)]");
  expect(markup).toContain(">2<");
  mockQueueState.queue = [];
});

test("uses closed-state hover text token", () => {
  mockQueueState.isOpen = false;
  const markup = renderToStaticMarkup(<QueueButton />);
  expect(markup).toContain("hover:text-[var(--color-text)]");
});
