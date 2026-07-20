import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test, vi } from "vitest";
import { RotationControls } from "./RotationControls";

const { mockRotationState, mockQueueState } = vi.hoisted(() => ({
  mockRotationState: {
    singerNames: ["David", "John", "Jack"],
    filterSinger: "John",
    queueSingers: new Map<string, string | null>(),
    addSinger: vi.fn(),
    removeSinger: vi.fn(),
    shuffleQueue: vi.fn(),
    setFilterSinger: vi.fn(),
  },
  mockQueueState: {
    queue: ["song-1", "song-2"],
    removeFromQueue: vi.fn(),
  },
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

vi.mock("@/stores/rotation-store", () => ({
  useRotationStore: (selector?: (s: typeof mockRotationState) => unknown) =>
    selector ? selector(mockRotationState) : mockRotationState,
}));

vi.mock("@/stores/queue-store", () => ({
  useQueueStore: (selector?: (s: typeof mockQueueState) => unknown) =>
    selector ? selector(mockQueueState) : mockQueueState,
}));

describe("RotationControls", () => {
  test("renders singer tags and shuffle button", () => {
    const markup = renderToStaticMarkup(<RotationControls />);

    expect(markup).toContain("David");
    expect(markup).toContain("John");
    expect(markup).toContain("Jack");
    expect(markup).toContain("rotation.shuffle");
    expect(markup).toContain('aria-pressed="true"');
  });

  test("shuffle button is in the header row", () => {
    const markup = renderToStaticMarkup(<RotationControls />);

    const headerDivMatch = markup.match(
      /<div class="flex items-center justify-between">(.*?)<\/div>/s,
    );
    expect(headerDivMatch).not.toBeNull();
    const headerContent = headerDivMatch![1];
    expect(headerContent).toContain("rotation.shuffle");
  });

  test("shows add singer button when no singers exist", () => {
    mockRotationState.singerNames = [];
    const markup = renderToStaticMarkup(<RotationControls />);
    expect(markup).toContain("+ Add Singer");
    mockRotationState.singerNames = ["David", "John", "Jack"];
  });
});
