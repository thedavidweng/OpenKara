// @vitest-environment jsdom
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { useCatalogStore } from "@/stores/catalog-store";
import { useSettingsStore } from "@/stores/settings-store";
import { YoutubeLinkPopover } from "./YoutubeLinkPopover";

vi.mock("react-i18next", () => ({
  initReactI18next: { type: "3rdParty", init: () => {} },
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock("@/components/Catalog/YoutubePasteLink", () => ({
  YoutubePasteLink: ({
    inputRef,
  }: {
    inputRef?: React.RefObject<HTMLInputElement | null>;
  }) => <input ref={inputRef} aria-label="youtube.pasteLink" />,
}));

beforeEach(() => {
  useCatalogStore.setState({ youtubeLinkOpen: true });
  useSettingsStore.setState({ youtubeSourceEnabled: true });
});

afterEach(cleanup);

describe("YoutubeLinkPopover", () => {
  test("renders nothing while closed or while the source is off", () => {
    useCatalogStore.setState({ youtubeLinkOpen: false });
    const { rerender } = render(<YoutubeLinkPopover />);
    expect(screen.queryByRole("dialog")).toBeNull();

    useCatalogStore.setState({ youtubeLinkOpen: true });
    useSettingsStore.setState({ youtubeSourceEnabled: false });
    rerender(<YoutubeLinkPopover />);
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  test("is a labelled dialog with the paste form", () => {
    render(<YoutubeLinkPopover />);
    const dialog = screen.getByRole("dialog", { name: "youtube.popoverTitle" });
    expect(dialog).toBeTruthy();
    expect(screen.getByLabelText("youtube.pasteLink")).toBeTruthy();
  });

  test("Escape closes it and returns focus to the trigger", () => {
    const trigger = document.createElement("button");
    trigger.setAttribute("data-youtube-link-trigger", "");
    document.body.appendChild(trigger);
    render(<YoutubeLinkPopover />);
    act(() => {
      fireEvent.keyDown(document, { key: "Escape" });
    });
    expect(useCatalogStore.getState().youtubeLinkOpen).toBe(false);
    expect(document.activeElement).toBe(trigger);
    trigger.remove();
  });

  test("a mousedown outside closes it but one inside does not", () => {
    render(<YoutubeLinkPopover />);
    fireEvent.mouseDown(screen.getByRole("dialog"));
    expect(useCatalogStore.getState().youtubeLinkOpen).toBe(true);
    fireEvent.mouseDown(document.body);
    expect(useCatalogStore.getState().youtubeLinkOpen).toBe(false);
  });
});
