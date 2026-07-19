// @vitest-environment jsdom

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { SettingsDialogHost } from "./SettingsDialogHost";
import {
  SettingsOverlayContext,
  createSettingsOverlayTestContextValue,
  type SettingsOverlayContextValue,
} from "./SettingsOverlay.context";

vi.mock("react-i18next", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react-i18next")>();
  return {
    ...actual,
    useTranslation: () => ({
      t: (key: string) => key,
      i18n: { changeLanguage: vi.fn() },
    }),
  };
});

function renderWithContext(
  node: ReactElement,
  value: SettingsOverlayContextValue,
) {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);
  act(() => {
    root.render(
      <SettingsOverlayContext value={value}>{node}</SettingsOverlayContext>,
    );
  });
  return { container, root };
}

describe("SettingsDialogHost interactions", () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    (
      globalThis as typeof globalThis & {
        IS_REACT_ACT_ENVIRONMENT?: boolean;
      }
    ).IS_REACT_ACT_ENVIRONMENT = true;
  });

  afterEach(() => {
    act(() => root.unmount());
    container.remove();
    // Clean up any portal content left on document.body
    document.body.innerHTML = "";
  });

  test("confirm button in integrity cleanup dialog calls confirmIntegrityCleanup", () => {
    const confirmIntegrityCleanup = vi.fn().mockResolvedValue(undefined);
    const value = createSettingsOverlayTestContextValue(
      {
        state: { integritySelection: new Set(["hash-a"]) },
        meta: { dangerDialog: "integrity_cleanup_confirm" },
      },
      { confirmIntegrityCleanup, closeDialog: vi.fn() },
    );

    const rendered = renderWithContext(<SettingsDialogHost />, value);
    container = rendered.container;
    root = rendered.root;

    // ConfirmationDialog portals to document.body.
    const confirmButton = document.body.querySelector(
      "button:not(:first-of-type)",
    ) as HTMLButtonElement;
    expect(confirmButton).not.toBeNull();
    // The confirm button is the second button (after cancel).
    const buttons = document.body.querySelectorAll("button");
    const confirmBtn = buttons[buttons.length - 1] as HTMLButtonElement;

    act(() => {
      confirmBtn.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });

    expect(confirmIntegrityCleanup).toHaveBeenCalledOnce();
  });
});
