import { Moon, Sun, Languages } from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";
import faqEn from "../faq.md?raw";
import privacyEn from "../privacy.md?raw";
import termsEn from "../terms.md?raw";
import faqZh from "../zh/faq.md?raw";
import privacyZh from "../zh/privacy.md?raw";
import termsZh from "../zh/terms.md?raw";
import {
  documentHref,
  withBase,
  type DocumentRoute,
  type DocumentSlug,
  type SiteLanguage,
} from "./document-route";

type Theme = "dark" | "light";

type MarkdownBlock =
  | { type: "heading"; level: 1 | 2 | 3; value: string }
  | { type: "paragraph"; value: string }
  | { type: "list"; values: string[] }
  | { type: "code"; value: string };

const CONTENT: Record<DocumentSlug, Record<SiteLanguage, string>> = {
  faq: { en: faqEn, "zh-CN": faqZh },
  privacy: { en: privacyEn, "zh-CN": privacyZh },
  terms: { en: termsEn, "zh-CN": termsZh },
};

const LABELS: Record<DocumentSlug, Record<SiteLanguage, string>> = {
  faq: { en: "FAQ", "zh-CN": "常见问题" },
  privacy: { en: "Privacy Policy", "zh-CN": "隐私政策" },
  terms: { en: "Terms of Service", "zh-CN": "服务条款" },
};

function parseMarkdown(markdown: string): MarkdownBlock[] {
  const source = markdown.replace(/^---\r?\n[\s\S]*?\r?\n---\r?\n/, "").trim();
  const blocks: MarkdownBlock[] = [];
  const paragraph: string[] = [];
  const list: string[] = [];
  let code: string[] | null = null;

  const flushParagraph = () => {
    if (paragraph.length > 0) {
      blocks.push({ type: "paragraph", value: paragraph.join(" ") });
      paragraph.length = 0;
    }
  };

  const flushList = () => {
    if (list.length > 0) {
      blocks.push({ type: "list", values: [...list] });
      list.length = 0;
    }
  };

  for (const line of source.split(/\r?\n/)) {
    if (line.startsWith("```")) {
      flushParagraph();
      flushList();
      if (code) {
        blocks.push({ type: "code", value: code.join("\n") });
        code = null;
      } else {
        code = [];
      }
      continue;
    }

    if (code) {
      code.push(line);
      continue;
    }

    const heading = /^(#{1,3})\s+(.+)$/.exec(line);
    if (heading) {
      flushParagraph();
      flushList();
      blocks.push({
        type: "heading",
        level: heading[1].length as 1 | 2 | 3,
        value: heading[2],
      });
      continue;
    }

    const listItem = /^-\s+(.+)$/.exec(line);
    if (listItem) {
      flushParagraph();
      list.push(listItem[1]);
      continue;
    }

    if (line.trim() === "") {
      flushParagraph();
      flushList();
      continue;
    }

    flushList();
    paragraph.push(line.trim());
  }

  flushParagraph();
  flushList();

  return blocks;
}

function normalizeInternalHref(href: string) {
  if (href.startsWith("/") && !href.endsWith("/") && !href.includes(".")) {
    return `${href}/`;
  }

  return href;
}

function renderInline(value: string): ReactNode[] {
  return value
    .split(/(\[[^\]]+\]\([^)]*\)|\*\*[^*]+\*\*|`[^`]+`)/g)
    .filter(Boolean)
    .map((part, index) => {
      const link = /^\[([^\]]+)\]\(([^)]*)\)$/.exec(part);
      if (link) {
        const normalized = normalizeInternalHref(link[2]);
        const external = /^https?:\/\//.test(normalized);
        // Internal root-relative hrefs must be prefixed with the Vite base
        // (/OpenKara/ in production) so document body links like
        // [Privacy Policy](/privacy) stay under the GitHub Pages project path.
        const href = external ? normalized : withBase(normalized);
        return (
          <a
            href={href}
            key={`link-${index}`}
            rel={external ? "noreferrer" : undefined}
            target={external ? "_blank" : undefined}
          >
            {link[1]}
          </a>
        );
      }

      if (part.startsWith("**") && part.endsWith("**")) {
        return <strong key={`strong-${index}`}>{part.slice(2, -2)}</strong>;
      }

      if (part.startsWith("`") && part.endsWith("`")) {
        return <code key={`code-${index}`}>{part.slice(1, -1)}</code>;
      }

      return part;
    });
}

export function DocumentPage({ language, slug }: DocumentRoute) {
  const [theme, setTheme] = useState<Theme>(() => {
    const saved = localStorage.getItem("openkara-site-theme");
    return saved === "light" ? "light" : "dark";
  });
  const label = LABELS[slug][language];
  const otherLanguage = language === "en" ? "zh-CN" : "en";
  const blocks = parseMarkdown(CONTENT[slug][language]);

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    document.documentElement.lang = language;
    document.title = `${label} | OpenKara`;
    localStorage.setItem("openkara-site-theme", theme);
    document
      .querySelector('meta[name="theme-color"]')
      ?.setAttribute("content", theme === "dark" ? "#08090a" : "#f7f7f5");
  }, [label, language, theme]);

  return (
    <div className="document-site">
      <header className="site-header">
        <nav className="site-nav" aria-label="Document navigation">
          <a className="brand" href={withBase("/")} aria-label="OpenKara home">
            <img src={withBase("/img/openkara-app-icon.png")} alt="" />
            <span>OpenKara</span>
          </a>
          <a className="document-home-link" href={withBase("/")}>
            {language === "en" ? "Back to home" : "返回首页"}
          </a>
          <div className="nav-actions">
            <a
              className="icon-button language-button"
              href={documentHref(otherLanguage, slug)}
              aria-label={
                language === "en" ? "切换到中文" : "Switch to English"
              }
            >
              <Languages size={16} />
              <span>{language === "en" ? "中文" : "EN"}</span>
            </a>
            <button
              className="icon-button theme-button"
              type="button"
              onClick={() => setTheme(theme === "dark" ? "light" : "dark")}
              aria-label={
                theme === "dark"
                  ? language === "en"
                    ? "Switch to light theme"
                    : "切换到浅色主题"
                  : language === "en"
                    ? "Switch to dark theme"
                    : "切换到深色主题"
              }
            >
              {theme === "dark" ? <Moon size={17} /> : <Sun size={17} />}
            </button>
          </div>
        </nav>
      </header>

      <main className="document-main content-width">
        <div className="document-breadcrumb" aria-label="Breadcrumb">
          <a href={withBase("/")}>OpenKara</a>
          <span>/</span>
          <span>{label}</span>
        </div>
        <article className="document-content">
          {blocks.map((block, index) => {
            if (block.type === "heading") {
              const Heading = `h${block.level}` as "h1" | "h2" | "h3";
              return <Heading key={`heading-${index}`}>{block.value}</Heading>;
            }

            if (block.type === "list") {
              return (
                <ul key={`list-${index}`}>
                  {block.values.map((item, itemIndex) => (
                    <li key={`${item}-${itemIndex}`}>{renderInline(item)}</li>
                  ))}
                </ul>
              );
            }

            if (block.type === "code") {
              return (
                <pre key={`code-${index}`}>
                  <code>{block.value}</code>
                </pre>
              );
            }

            return (
              <p key={`paragraph-${index}`}>{renderInline(block.value)}</p>
            );
          })}
        </article>
      </main>

      <footer className="document-footer content-width">
        <a href={withBase("/")}>OpenKara</a>
        <div>
          <a href={documentHref(language, "faq")}>{LABELS.faq[language]}</a>
          <a href={documentHref(language, "privacy")}>
            {LABELS.privacy[language]}
          </a>
          <a href={documentHref(language, "terms")}>{LABELS.terms[language]}</a>
        </div>
      </footer>
    </div>
  );
}
