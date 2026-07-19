import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test } from "vitest";
import { SettingsSectionCard } from "./SettingsSectionCard";

describe("SettingsSectionCard", () => {
  test("renders the title and children", () => {
    const markup = renderToStaticMarkup(
      <SettingsSectionCard title="General">
        <p>card content</p>
      </SettingsSectionCard>,
    );

    expect(markup).toContain("General");
    expect(markup).toContain("card content");
  });

  test("renders the optional description", () => {
    const markup = renderToStaticMarkup(
      <SettingsSectionCard
        title="Library"
        description="Manage your song collection"
      >
        <p>body</p>
      </SettingsSectionCard>,
    );

    expect(markup).toContain("Manage your song collection");
  });

  test("omits the description paragraph when not provided", () => {
    const markup = renderToStaticMarkup(
      <SettingsSectionCard title="No Desc">
        <p>body</p>
      </SettingsSectionCard>,
    );

    expect(markup).toContain("No Desc");
    expect(markup).toContain("body");
  });

  test("applies danger styling when tone is danger", () => {
    const markup = renderToStaticMarkup(
      <SettingsSectionCard title="Danger Zone" tone="danger">
        <button>Delete Everything</button>
      </SettingsSectionCard>,
    );

    expect(markup).toContain("text-[var(--color-destructive)]");
  });

  test("applies default styling when tone is default", () => {
    const markup = renderToStaticMarkup(
      <SettingsSectionCard title="Safe Zone">
        <p>safe content</p>
      </SettingsSectionCard>,
    );

    expect(markup).toContain("border-[var(--color-border)]");
    expect(markup).toContain("font-semibold");
    expect(markup).toContain("text-[var(--color-text)]");
    expect(markup).not.toContain("text-[var(--color-destructive)]");
  });

  test("defaults to the default tone when tone is not specified", () => {
    const markup = renderToStaticMarkup(
      <SettingsSectionCard title="Implicit Default">
        <p>content</p>
      </SettingsSectionCard>,
    );

    expect(markup).toContain("border-[var(--color-border)]");
    expect(markup).not.toContain("text-[var(--color-destructive)]");
  });
});
