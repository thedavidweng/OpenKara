// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { useCatalogStore } from "@/stores/catalog-store";
import { NeteaseSignInErrorBanner } from "./NeteaseSignInErrorBanner";

const { copyDebugInfo } = vi.hoisted(() => ({ copyDebugInfo: vi.fn() }));

vi.mock("react-i18next", () => ({
  initReactI18next: { type: "3rdParty", init: () => {} },
  useTranslation: () => ({
    t: (key: string, options?: { message?: string }) =>
      options?.message ? `${key}: ${options.message}` : key,
  }),
}));
vi.mock("@/lib/debug-info", () => ({ copyDebugInfo }));
vi.mock("@/lib/errors", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/errors")>()),
  notifyError: vi.fn(),
}));

beforeEach(() => {
  copyDebugInfo.mockReset();
  copyDebugInfo.mockResolvedValue(undefined);
  useCatalogStore.setState({ signInError: null });
});

afterEach(cleanup);

describe("NeteaseSignInErrorBanner", () => {
  test("renders nothing without an error", () => {
    render(<NeteaseSignInErrorBanner />);
    expect(screen.queryByRole("alert")).toBeNull();
  });

  test("shows a single alert for the latest error and dismisses it", () => {
    const error = {
      code: "internal" as const,
      message: "window closed",
      retryable: false,
      fallback: "retry" as const,
    };
    useCatalogStore.setState({ signInError: error });
    render(<NeteaseSignInErrorBanner />);
    expect(screen.getAllByRole("alert")).toHaveLength(1);
    expect(screen.getByRole("alert").textContent).toContain("window closed");
    fireEvent.click(screen.getByRole("button", { name: "common.close" }));
    expect(useCatalogStore.getState().signInError).toBeNull();
  });

  test("copies debug info", async () => {
    useCatalogStore.setState({
      signInError: {
        code: "internal",
        message: "nope",
        retryable: false,
        fallback: "retry",
      },
    });
    render(<NeteaseSignInErrorBanner />);
    fireEvent.click(
      screen.getByRole("button", { name: "settings.about.copyDebugInfo" }),
    );
    await screen.findByText("settings.about.copied");
    expect(copyDebugInfo).toHaveBeenCalledTimes(1);
  });
});
