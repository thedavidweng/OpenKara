import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test, vi } from "vitest";
import { RotationControls } from "./RotationControls";

const { mockRotationState } = vi.hoisted(() => ({
  mockRotationState: {
    active: true,
    singerNames: ["David", "John", "Jack"],
    currentIndex: 1,
    mode: "round_robin" as const,
    queueSingers: new Map<string, string | null>(),
    isLoading: false,
    loadRotation: vi.fn(),
    toggleActive: vi.fn(),
    addSinger: vi.fn(),
    removeSinger: vi.fn(),
    advanceRotation: vi.fn(),
    setCurrentSinger: vi.fn(),
    assignSingerToQueueEntry: vi.fn(),
    getNextSinger: vi.fn(),
  },
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

vi.mock("@/stores/rotation-store", () => ({
  useRotationStore: () => mockRotationState,
}));

describe("RotationControls", () => {
  test("renders the current singer selector and next singer action", () => {
    const markup = renderToStaticMarkup(<RotationControls />);

    expect(markup).toContain("David");
    expect(markup).toContain("John");
    expect(markup).toContain("Jack");
    expect(markup).toContain("rotation.nextSinger");
    expect(markup).toContain('aria-pressed="true"');
  });
});
