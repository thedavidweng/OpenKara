// @vitest-environment jsdom

import { cleanup } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { SortModeSelector } from "./SortModeSelector";

const { mockUseSettingsStore, mockUseTranslation, mockSetLibrarySortMode } =
  vi.hoisted(() => ({
    mockUseSettingsStore: vi.fn(),
    mockUseTranslation: vi.fn(() => ({
      t: (key: string) => key,
    })),
    mockSetLibrarySortMode: vi.fn(),
  }));

vi.mock("@/stores/settings-store", () => ({
  useSettingsStore: mockUseSettingsStore,
}));

vi.mock("react-i18next", () => ({
  useTranslation: mockUseTranslation,
}));

function setupStore(
  mode: string,
  setMode: ReturnType<typeof vi.fn> = mockSetLibrarySortMode,
) {
  mockUseSettingsStore.mockImplementation((selector: (s: unknown) => unknown) =>
    selector({
      librarySortMode: mode,
      setLibrarySortMode: setMode,
    }),
  );
}

describe("SortModeSelector", () => {
  beforeEach(() => {
    cleanup();
    mockUseSettingsStore.mockReset();
    mockUseTranslation.mockReset();
    mockSetLibrarySortMode.mockReset();
    mockUseTranslation.mockReturnValue({
      t: (key: string) => key,
    });
    mockSetLibrarySortMode.mockResolvedValue(undefined);
  });

  afterEach(() => {
    cleanup();
  });

  test("renders a select with the current mode value", () => {
    setupStore("recently_imported");
    render(<SortModeSelector />);
    const select = screen.getByTestId(
      "sort-mode-selector",
    ) as HTMLSelectElement;
    expect(select.value).toBe("recently_imported");
  });

  test("renders all three mode options", () => {
    setupStore("title_asc");
    render(<SortModeSelector />);
    const select = screen.getByTestId("sort-mode-selector");
    const options = select.querySelectorAll("option");
    expect(options).toHaveLength(3);
    expect((options[0] as HTMLOptionElement).value).toBe("recently_imported");
    expect((options[1] as HTMLOptionElement).value).toBe("title_asc");
    expect((options[2] as HTMLOptionElement).value).toBe("artist_asc");
  });

  test("calls setLibrarySortMode when a different option is selected", () => {
    setupStore("recently_imported");
    render(<SortModeSelector />);
    const select = screen.getByTestId("sort-mode-selector");
    fireEvent.change(select, { target: { value: "artist_asc" } });
    expect(mockSetLibrarySortMode).toHaveBeenCalledWith("artist_asc");
  });

  test("does not call setLibrarySortMode when the same option is reselected", () => {
    setupStore("title_asc");
    render(<SortModeSelector />);
    const select = screen.getByTestId("sort-mode-selector");
    fireEvent.change(select, { target: { value: "title_asc" } });
    expect(mockSetLibrarySortMode).not.toHaveBeenCalled();
  });

  test("disables the select while a mutation is pending", async () => {
    let resolveMutation: () => void = () => {};
    mockSetLibrarySortMode.mockImplementation(
      () =>
        new Promise<void>((resolve) => {
          resolveMutation = resolve;
        }),
    );
    setupStore("recently_imported");
    render(<SortModeSelector />);
    const select = screen.getByTestId(
      "sort-mode-selector",
    ) as HTMLSelectElement;
    fireEvent.change(select, { target: { value: "title_asc" } });
    // setIsPending(true) runs inside the change handler's first await; wait
    // for React to commit that re-render rather than assuming a synchronous
    // flush (jsdom schedules it as a microtask, which races under load).
    await screen.findByTestId("sort-mode-selector");
    expect(select.disabled).toBe(true);
    resolveMutation();
    await screen.findByTestId("sort-mode-selector");
    expect(select.disabled).toBe(false);
  });
});
