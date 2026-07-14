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
