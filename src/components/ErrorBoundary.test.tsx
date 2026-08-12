// @vitest-environment jsdom

import { screen } from "@testing-library/react";
import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterAll, beforeEach, describe, expect, test, vi } from "vitest";
import { createMockBackend } from "@/lib/backend/mock-backend";
import { renderWithBackend } from "@/test-utils/backend";
import { ErrorBoundary } from "./ErrorBoundary";

const mockWindowReady = vi.fn(() => Promise.resolve());
const backend = createMockBackend({
  overrides: { settings: { windowReady: mockWindowReady } },
});

function render(ui: ReactElement) {
  return renderWithBackend(ui, backend);
}

const consoleSpy = vi.spyOn(console, "error").mockImplementation(() => {});
afterAll(() => {
  consoleSpy.mockRestore();
});

beforeEach(() => {
  mockWindowReady.mockClear();
});

function Boom(): never {
  throw new Error("kaboom");
}

describe("ErrorBoundary", () => {
  test("renders children when no error is thrown", () => {
    const markup = renderToStaticMarkup(
      <ErrorBoundary>
        <div>child content</div>
      </ErrorBoundary>,
    );

    expect(markup).toContain("child content");
    expect(markup).not.toContain("Something went wrong");
  });

  test("getDerivedStateFromError produces an error state with the message", () => {
    const state = ErrorBoundary.getDerivedStateFromError(new Error("kaboom"));

    expect(state).toEqual({ hasError: true, error: expect.any(Error) });
    expect(state.error!.message).toBe("kaboom");
  });

  test("getDerivedStateFromError sets hasError to true", () => {
    const state = ErrorBoundary.getDerivedStateFromError(new Error("any"));

    expect(state.hasError).toBe(true);
  });

  test("starts with hasError false in the initial state", () => {
    const instance = new ErrorBoundary({ children: null });

    expect(instance.state.hasError).toBe(false);
    expect(instance.state.error).toBeNull();
  });

  test("reveals the hidden main window when a child crashes", () => {
    render(
      <ErrorBoundary>
        <Boom />
      </ErrorBoundary>,
    );

    expect(screen.getByText("Something went wrong")).toBeTruthy();
    expect(mockWindowReady).toHaveBeenCalledTimes(1);
  });

  test("does not touch the window on a healthy render", () => {
    render(
      <ErrorBoundary>
        <div>child content</div>
      </ErrorBoundary>,
    );

    expect(mockWindowReady).not.toHaveBeenCalled();
  });
});
