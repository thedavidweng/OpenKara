// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, expect, test, vi } from "vite-plus/test";
import { SettingsOverlay } from "./SettingsOverlay";

vi.mock("react-i18next", () => ({
  initReactI18next: { type: "3rdParty", init: () => {} },
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { language: "en" },
  }),
}));

vi.mock("./SettingsLibrarySection", () => ({
  SettingsLibrarySection: () => <div>library section</div>,
}));
vi.mock("./SettingsOnlineSourcesSection", () => ({
  SettingsOnlineSourcesSection: () => <div>sources section</div>,
}));
vi.mock("./SettingsController.provider", () => ({
  SettingsControllerProvider: ({ children }: { children: ReactNode }) =>
    children,
}));
vi.mock("./SettingsDialogHost", () => ({ SettingsDialogHost: () => null }));

afterEach(cleanup);

test("the page nav shows one page at a time and moves with arrow keys", () => {
  render(<SettingsOverlay />);
  const library = screen.getByRole("tab", { name: "settings.pages.library" });
  expect(library.getAttribute("aria-selected")).toBe("true");
  expect(screen.getByText("library section")).toBeTruthy();
  expect(screen.queryByText("sources section")).toBeNull();

  fireEvent.keyDown(library, { key: "ArrowDown" });
  const sources = screen.getByRole("tab", {
    name: "settings.pages.onlineSources",
  });
  expect(sources.getAttribute("aria-selected")).toBe("true");
  expect(document.activeElement).toBe(sources);
  expect(screen.getByText("sources section")).toBeTruthy();
  expect(screen.queryByText("library section")).toBeNull();
});
