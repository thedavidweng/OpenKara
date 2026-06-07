import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test, vi } from "vitest";
import { EmptyLibrary } from "./EmptyLibrary";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

vi.mock("./ImportButton", () => ({
  ImportButton: ({ children }: { children: React.ReactNode }) => (
    <div data-testid="import-button">{children}</div>
  ),
}));

describe("EmptyLibrary", () => {
  test("renders the no-tracks message and import button", () => {
    const markup = renderToStaticMarkup(<EmptyLibrary />);

    expect(markup).toContain("library.noTracks");
    expect(markup).toContain("library.importMusic");
  });

  test("wraps the import action in the ImportButton component", () => {
    const markup = renderToStaticMarkup(<EmptyLibrary />);

    expect(markup).toContain('data-testid="import-button"');
  });
});
