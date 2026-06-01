import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test, vi } from "vitest";
import { SingerPickerDialog } from "./SingerPickerDialog";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

describe("SingerPickerDialog", () => {
  test("renders singer names as buttons, not a text input", () => {
    const markup = renderToStaticMarkup(
      <SingerPickerDialog
        singerNames={["David", "Leo", "Jack"]}
        currentSinger={null}
        onSelect={vi.fn()}
        onRemove={vi.fn()}
        onCancel={vi.fn()}
      />,
    );

    expect(markup).toContain("David");
    expect(markup).toContain("Leo");
    expect(markup).toContain("Jack");
    expect(markup).not.toContain("<input");
  });

  test("highlights the currently assigned singer", () => {
    const markup = renderToStaticMarkup(
      <SingerPickerDialog
        singerNames={["David", "Leo"]}
        currentSinger="Leo"
        onSelect={vi.fn()}
        onRemove={vi.fn()}
        onCancel={vi.fn()}
      />,
    );

    expect(markup).toContain("rotation.assignSinger");
    expect(markup).toContain("rotation.removeSinger");
  });
});
