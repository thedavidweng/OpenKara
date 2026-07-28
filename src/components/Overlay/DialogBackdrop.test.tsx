// @vitest-environment jsdom

import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, test, vi } from "vitest";
import { DialogBackdrop } from "./DialogBackdrop";

describe("DialogBackdrop", () => {
  test("keeps pointer dismissal semantic while Escape remains the tab path", () => {
    const onDismiss = vi.fn();
    render(
      <DialogBackdrop
        ariaLabel="Close dialog"
        onDismiss={onDismiss}
        className="absolute inset-0"
      />,
    );

    const backdrop = screen.getByRole("button", { name: "Close dialog" });
    expect(backdrop.getAttribute("tabindex")).toBe("-1");
    fireEvent.click(backdrop);
    expect(onDismiss).toHaveBeenCalledOnce();
  });
});
