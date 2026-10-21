// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { useCatalogStore, type ImportRun } from "@/stores/catalog-store";
import { StreamingImportBar } from "./StreamingImportBar";

vi.mock("react-i18next", () => ({
  initReactI18next: { type: "3rdParty", init: () => {} },
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) =>
      options ? `${key}|${JSON.stringify(options)}` : key,
  }),
}));

const runOf = (overrides: Partial<ImportRun>): ImportRun => ({
  sourceId: "alpha",
  jobId: "job",
  phase: "running",
  total: 1,
  completed: 0,
  imported: 0,
  skipped: 0,
  label: null,
  cancelling: false,
  ...overrides,
});

beforeEach(() => {
  useCatalogStore.setState({ importRun: null, importFailures: [] });
});

afterEach(cleanup);

describe("StreamingImportBar", () => {
  test("renders nothing without a run", () => {
    render(<StreamingImportBar />);
    expect(screen.queryByTestId("streaming-import-bar")).toBeNull();
  });

  test("shows determinate progress, the name and a cancel button while running", () => {
    const cancelImport = vi.fn();
    useCatalogStore.setState({
      importRun: runOf({ total: 4, completed: 1, label: "Night set" }),
      cancelImport,
    });
    render(<StreamingImportBar />);
    const bar = screen.getByRole("progressbar");
    expect(bar.getAttribute("aria-valuenow")).toBe("1");
    expect(bar.getAttribute("aria-valuemax")).toBe("4");
    expect(
      screen.getByText(
        'catalog.streaming.importingProgress|{"completed":1,"total":4}',
      ),
    ).toBeTruthy();
    fireEvent.click(
      screen.getByRole("button", { name: "catalog.streaming.cancelImport" }),
    );
    expect(cancelImport).toHaveBeenCalledTimes(1);
  });

  test("shows the skipped count and a View skipped list while running", () => {
    useCatalogStore.setState({
      importRun: runOf({ total: 3, completed: 1, skipped: 1 }),
      importFailures: [
        {
          remote_track_id: "9",
          title: "Grey",
          artist: "C",
          reason: "refusal",
          refusal: { reason: "no_play_rights", title: "Grey", artist: "C" },
        },
      ],
    });
    render(<StreamingImportBar />);
    expect(
      screen.getByText('catalog.streaming.skippedSoFar|{"skipped":1}'),
    ).toBeTruthy();
    fireEvent.click(
      screen.getByRole("button", { name: "catalog.streaming.viewSkipped" }),
    );
    expect(screen.getByText(/Grey/)).toBeTruthy();
  });

  test("disables cancel once cancelling", () => {
    useCatalogStore.setState({ importRun: runOf({ cancelling: true }) });
    render(<StreamingImportBar />);
    expect(screen.getByText("catalog.streaming.cancelling")).toBeTruthy();
    expect(
      (
        screen.getByRole("button", {
          name: "catalog.streaming.cancelImport",
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(true);
  });

  test("summarizes results, toggles skipped tracks and dismisses", () => {
    useCatalogStore.setState({
      importRun: runOf({
        phase: "done",
        total: 2,
        completed: 2,
        imported: 1,
        skipped: 1,
      }),
      importFailures: [
        {
          remote_track_id: "9",
          title: "Grey",
          artist: "C",
          reason: "refusal",
          refusal: { reason: "no_play_rights", title: "Grey", artist: "C" },
        },
      ],
    });
    render(<StreamingImportBar />);
    expect(screen.getByText(/catalog.streaming.importDone/)).toBeTruthy();
    expect(screen.queryByText(/Grey/)).toBeNull();
    const toggle = screen.getByRole("button", {
      name: "catalog.streaming.viewSkipped",
    });
    fireEvent.click(toggle);
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    expect(screen.getByText(/Grey/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "common.close" }));
    expect(useCatalogStore.getState().importRun).toBeNull();
  });

  test("names the refusal reason and falls back to the id for unnamed songs", () => {
    useCatalogStore.setState({
      importRun: runOf({ phase: "done", total: 1, completed: 1, skipped: 1 }),
      importFailures: [
        {
          remote_track_id: "426194887",
          title: "",
          artist: "",
          reason: "refusal",
          refusal: { reason: "trial_clip", title: "", artist: "" },
        },
      ],
    });
    render(<StreamingImportBar />);
    fireEvent.click(
      screen.getByRole("button", { name: "catalog.streaming.viewSkipped" }),
    );
    expect(screen.getByText("426194887")).toBeTruthy();
    expect(
      screen.getByText(/catalog.streaming.refusalReason.trial_clip/),
    ).toBeTruthy();
  });
});
