export type SiteLanguage = "en" | "zh-CN";
export type DocumentSlug = "faq" | "privacy" | "terms";

export interface DocumentRoute {
  language: SiteLanguage;
  slug: DocumentSlug;
}

export function documentHref(language: SiteLanguage, slug: DocumentSlug) {
  return language === "zh-CN" ? `/zh/${slug}/` : `/${slug}/`;
}

export function getDocumentRoute(pathname: string): DocumentRoute | null {
  const segments = pathname.split("/").filter(Boolean);
  const slug = segments[segments.length - 1];

  if (slug !== "faq" && slug !== "privacy" && slug !== "terms") {
    return null;
  }

  return {
    language: segments[segments.length - 2] === "zh" ? "zh-CN" : "en",
    slug,
  };
}
