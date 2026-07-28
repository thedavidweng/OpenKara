// @vitest-environment jsdom

import { useRef, useState } from "react";
import { afterEach, describe, expect, test, vi } from "vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { useModalDialog } from "./use-modal-dialog";

function TestDialog({
  label,
  onDismiss,
}: {
  label: string;
  onDismiss: () => void;
}) {
  const dialogRef = useRef<HTMLDivElement>(null);
  const initialFocusRef = useRef<HTMLButtonElement>(null);
  useModalDialog({ dialogRef, initialFocusRef, onDismiss });

  return (
    <div ref={dialogRef} role="dialog" tabIndex={-1} aria-label={label}>
      <button ref={initialFocusRef}>First {label}</button>
      <button>Last {label}</button>
    </div>
  );
}

afterEach(cleanup);

describe("useModalDialog", () => {
  test("sets initial focus and wraps Tab in the dialog", async () => {
    render(<TestDialog label="test dialog" onDismiss={vi.fn()} />);

    const first = screen.getByRole("button", { name: "First test dialog" });
    const last = screen.getByRole("button", { name: "Last test dialog" });
    await waitFor(() => expect(document.activeElement).toBe(first));

    fireEvent.keyDown(document, { key: "Tab", shiftKey: true });
    expect(document.activeElement).toBe(last);

    fireEvent.keyDown(document, { key: "Tab" });
    expect(document.activeElement).toBe(first);
  });

  test("only dismisses the topmost nested dialog with Escape", async () => {
    const lowerDismiss = vi.fn();
    const upperDismiss = vi.fn();
    render(
      <>
        <TestDialog label="lower dialog" onDismiss={lowerDismiss} />
        <TestDialog label="upper dialog" onDismiss={upperDismiss} />
      </>,
    );

    await waitFor(() =>
      expect(document.activeElement).toBe(
        screen.getByRole("button", { name: "First upper dialog" }),
      ),
    );
    fireEvent.keyDown(document, { key: "Escape" });

    expect(upperDismiss).toHaveBeenCalledOnce();
    expect(lowerDismiss).not.toHaveBeenCalled();
  });

  test("restores focus to the invoking control after close", async () => {
    function Harness() {
      const [open, setOpen] = useState(false);
      return (
        <>
          <button onClick={() => setOpen(true)}>Open dialog</button>
          {open && (
            <TestDialog
              label="restored dialog"
              onDismiss={() => setOpen(false)}
            />
          )}
        </>
      );
    }

    render(<Harness />);
    const trigger = screen.getByRole("button", { name: "Open dialog" });
    trigger.focus();
    fireEvent.click(trigger);

    await waitFor(() =>
      expect(document.activeElement).toBe(
        screen.getByRole("button", { name: "First restored dialog" }),
      ),
    );
    fireEvent.keyDown(document, { key: "Escape" });

    await waitFor(() => expect(document.activeElement).toBe(trigger));
  });
});
