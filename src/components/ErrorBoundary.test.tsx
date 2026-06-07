import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test, vi } from "vitest";
import { ErrorBoundary } from "./ErrorBoundary";

vi.spyOn(console, "error").mockImplementation(() => {});

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
    const state = (ErrorBoundary as any).getDerivedStateFromError(
      new Error("kaboom"),
    );

    expect(state).toEqual({ hasError: true, error: expect.any(Error) });
    expect(state.error.message).toBe("kaboom");
  });

  test("getDerivedStateFromError sets hasError to true", () => {
    const state = (ErrorBoundary as any).getDerivedStateFromError(
      new Error("any"),
    );

    expect(state.hasError).toBe(true);
  });

  test("starts with hasError false in the initial state", () => {
    const instance = new ErrorBoundary({ children: null });

    expect(instance.state.hasError).toBe(false);
    expect(instance.state.error).toBeNull();
  });
});
