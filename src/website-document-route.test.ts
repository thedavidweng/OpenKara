import { describe, expect, test } from "vitest";
import { documentHref, getDocumentRoute } from "../website/src/document-route";

describe("website document routes", () => {
  test("maps the published English and Chinese document paths", () => {
    expect(getDocumentRoute("/privacy/")).toEqual({
      language: "en",
      slug: "privacy",
    });
    expect(getDocumentRoute("/zh/terms/")).toEqual({
      language: "zh-CN",
      slug: "terms",
    });
  });

  test("keeps non-document routes on the landing page", () => {
    expect(getDocumentRoute("/")).toBeNull();
    expect(getDocumentRoute("/features/")).toBeNull();
  });

  test("builds static-document links for each language", () => {
    expect(documentHref("en", "faq")).toBe("/faq/");
    expect(documentHref("zh-CN", "privacy")).toBe("/zh/privacy/");
  });
});
