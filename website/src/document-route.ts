export type SiteLanguage = "en" | "zh-CN";
export type DocumentSlug = "faq" | "privacy" | "terms";

export interface DocumentRoute {
  language: SiteLanguage;
  slug: DocumentSlug;
}

/**
 * Prefix a root-absolute path with the Vite base so assets and internal links
 * resolve correctly when the site is served from a subpath (e.g. the GitHub
 * Pages project URL `thedavidweng.github.io/OpenKara/`). In dev `BASE_URL` is
 * `/`, so this is a no-op; in production it is `/OpenKara/`.
 */
export function withBase(path: string): string {
  if (!path.startsWith("/")) return path;
  return `${import.meta.env.BASE_URL}${path.slice(1)}`;
}

export function documentHref(language: SiteLanguage, slug: DocumentSlug) {
  return withBase(language === "zh-CN" ? `/zh/${slug}/` : `/${slug}/`);
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
