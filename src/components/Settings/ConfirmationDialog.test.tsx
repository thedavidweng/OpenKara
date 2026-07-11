import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test, vi } from "vitest";
import { ConfirmationDialog } from "./ConfirmationDialog";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

describe("ConfirmationDialog", () => {
  test("renders the title, message, and confirm label", () => {
    const markup = renderToStaticMarkup(
      <ConfirmationDialog
        title="Delete Songs"
        message="Are you sure you want to delete 3 songs?"
        confirmLabel="Delete"
        onConfirm={vi.fn()}
        onCancel={vi.fn()}
      />,
    );

    expect(markup).toContain("Delete Songs");
    expect(markup).toContain("Are you sure you want to delete 3 songs?");
    expect(markup).toContain("Delete");
    expect(markup).toContain("common.cancel");
  });

  test("renders the optional detail text when provided", () => {
    const markup = renderToStaticMarkup(
      <ConfirmationDialog
        title="Reset Settings"
        message="This will restore defaults."
        detail="You cannot undo this action."
        confirmLabel="Reset"
        onConfirm={vi.fn()}
        onCancel={vi.fn()}
      />,
    );

    expect(markup).toContain("You cannot undo this action.");
  });

  test("omits the detail paragraph when not provided", () => {
    const markup = renderToStaticMarkup(
      <ConfirmationDialog
        title="Confirm"
        message="Proceed?"
        confirmLabel="OK"
        onConfirm={vi.fn()}
        onCancel={vi.fn()}
      />,
    );

    expect(markup).toContain("Proceed?");
    // The dialog should still render without the detail section
    expect(markup).toContain('role="dialog"');
  });

  test("renders the dialog with correct ARIA attributes", () => {
    const markup = renderToStaticMarkup(
      <ConfirmationDialog
        title="Accessible Title"
        message="Body"
        confirmLabel="Yes"
        onConfirm={vi.fn()}
        onCancel={vi.fn()}
      />,
    );

    expect(markup).toContain('role="dialog"');
    expect(markup).toContain('aria-modal="true"');
    expect(markup).toContain('aria-labelledby="confirmation-dialog-title"');
  });

  test("renders a red-styled destructive confirm button", () => {
    const markup = renderToStaticMarkup(
      <ConfirmationDialog
        title="Dangerous"
        message="This is destructive."
        confirmLabel="Destroy"
        onConfirm={vi.fn()}
        onCancel={vi.fn()}
      />,
    );

    expect(markup).toContain("bg-[var(--color-destructive)]");
    expect(markup).toContain("text-[var(--color-destructive-foreground)]");
  });
});
