import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test, vi } from "vitest";
import { InputDialog } from "./InputDialog";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

describe("InputDialog", () => {
  test("renders the title and an empty text input", () => {
    const markup = renderToStaticMarkup(
      <InputDialog
        title="Create Playlist"
        placeholder="My Playlist"
        onConfirm={vi.fn()}
        onCancel={vi.fn()}
      />,
    );

    expect(markup).toContain("Create Playlist");
    expect(markup).toContain('type="text"');
    expect(markup).toContain('placeholder="My Playlist"');
    expect(markup).toContain("common.cancel");
    expect(markup).toContain("common.save");
  });

  test("uses the provided initial value", () => {
    const markup = renderToStaticMarkup(
      <InputDialog
        title="Rename"
        initialValue="Old Name"
        onConfirm={vi.fn()}
        onCancel={vi.fn()}
      />,
    );

    expect(markup).toContain('value="Old Name"');
  });

  test("uses a custom confirm label when provided", () => {
    const markup = renderToStaticMarkup(
      <InputDialog
        title="Add Singer"
        confirmLabel="Add"
        onConfirm={vi.fn()}
        onCancel={vi.fn()}
      />,
    );

    expect(markup).toContain("Add");
    expect(markup).not.toContain("common.save");
  });

  test("renders the dialog with correct ARIA attributes", () => {
    const markup = renderToStaticMarkup(
      <InputDialog
        title="Input Title"
        onConfirm={vi.fn()}
        onCancel={vi.fn()}
      />,
    );

    expect(markup).toContain('role="dialog"');
    expect(markup).toContain('aria-modal="true"');
    expect(markup).toContain('aria-labelledby="input-dialog-title"');
  });

  test("disables the confirm button when value is empty", () => {
    const markup = renderToStaticMarkup(
      <InputDialog title="Empty" onConfirm={vi.fn()} onCancel={vi.fn()} />,
    );

    expect(markup).toContain("disabled");
    expect(markup).toContain("disabled:opacity-50");
  });

  test("associates the input with the dialog title via aria-label", () => {
    const markup = renderToStaticMarkup(
      <InputDialog
        title="Playlist Name"
        onConfirm={vi.fn()}
        onCancel={vi.fn()}
      />,
    );

    expect(markup).toContain('aria-label="Playlist Name"');
  });
});
