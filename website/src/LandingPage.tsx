import { lazy, Suspense, useLayoutEffect, useState } from "react";
import {
  Code,
  Download,
  HardDrive,
  Languages,
  Moon,
  Play,
  SlidersHorizontal,
  Sparkles,
  Sun,
} from "lucide-react";
import { documentHref, withBase, type SiteLanguage } from "./document-route";

const AppPreview = lazy(() =>
  import("./AppPreview").then((module) => ({ default: module.AppPreview })),
);

type Language = SiteLanguage;
type Theme = "dark" | "light";

const COPY = {
  en: {
    nav: ["Features", "Workflow"],
    download: "Download",
    heroTitle: "Your music, on your stage.",
    heroBody:
      "OpenKara turns the songs you already own into a complete karaoke experience with on-device stem separation, synced lyrics, and precise mixing in one open-source desktop app.",
    primary: "Download free",
    secondary: "Watch demo",
    platform: "For macOS, Windows, and Linux",
    builtWith: "Built with",
    builtWithDisclaimer:
      "Tool names and logos are shown for identification only. No partnership, sponsorship, endorsement, or affiliation is implied.",
    sections: [
      {
        eyebrow: "AI stem separation",
        title: "Turn every song into karaoke.",
        body: "Separate vocals, drums, bass, and other instruments on your computer. Use two stems for a quick backing track or four for complete control.",
      },
      {
        eyebrow: "One complete library",
        title: "Import once. Sing anytime.",
        body: "OpenKara keeps metadata, stems, and lyrics together in a self-contained library that can live on your computer, NAS, USB drive, or in the cloud via Dropbox and Google Drive.",
      },
    ],
    cards: [
      [
        "2 or 4 stems",
        "Make a quick instrumental or control each part independently.",
      ],
      [
        "Live mixing",
        "Adjust every stem during playback without processing again.",
      ],
      [
        "Synced lyrics",
        "Use timed lyrics, embedded lyrics, LRC files, and CD+G.",
      ],
      [
        "Portable library",
        "Back up or move one self-contained library directory.",
      ],
    ],
    closing: "Turn your music into karaoke tonight.",
    source: "View source",
    footerProduct: "Product",
    footerProject: "Project",
    footerLegal: "Legal",
    footerFaq: "FAQ",
    footerSourceCode: "Source code",
    footerChangelog: "Changelog",
    footerLicense: "Apache 2.0 License",
    footerPrivacy: "Privacy Policy",
    footerTerms: "Terms of Service",
    themeDark: "Switch to dark theme",
    themeLight: "Switch to light theme",
  },
  "zh-CN": {
    nav: ["功能", "流程"],
    download: "下载",
    heroTitle: "你的音乐，你的舞台。",
    heroBody:
      "OpenKara 用端侧 AI 人声分离、同步歌词和精细混音，把你已经拥有的音乐变成完整的卡拉 OK 体验——全部集中在一个开源桌面应用里。",
    primary: "免费下载",
    secondary: "观看演示",
    platform: "支持 macOS、Windows 和 Linux",
    builtWith: "构建工具",
    builtWithDisclaimer:
      "工具名称与图标仅用于识别，不代表任何合作、赞助、背书或关联关系。",
    sections: [
      {
        eyebrow: "AI 人声分离",
        title: "每首歌都能唱。",
        body: "在本地分离人声、鼓、贝斯和其他乐器。双轨模式快速拿到伴奏，四轨模式逐轨控制。",
      },
      {
        eyebrow: "一个完整曲库",
        title: "导入一次，随时开唱。",
        body: "OpenKara 把元数据、分轨和歌词集中在一个自包含曲库里，可以放在电脑、NAS、U 盘上，或通过 Dropbox、Google Drive 存到云端。",
      },
    ],
    cards: [
      ["2 轨或 4 轨", "想要快速伴奏，还是逐轨独立控制，都行。"],
      ["实时混音", "播放时直接调各轨音量，不用重新处理。"],
      ["同步歌词", "支持时间同步歌词、内嵌歌词、.lrc 文件和 CD+G。"],
      ["可移植曲库", "一个自包含目录就能备份或搬到别的设备。"],
    ],
    closing: "今晚就把你的音乐变成卡拉 OK。",
    source: "查看源代码",
    footerProduct: "产品",
    footerProject: "项目",
    footerLegal: "法律",
    footerFaq: "常见问题",
    footerSourceCode: "源代码",
    footerChangelog: "更新日志",
    footerLicense: "Apache 2.0 许可",
    footerPrivacy: "隐私政策",
    footerTerms: "服务条款",
    themeDark: "切换到深色主题",
    themeLight: "切换到浅色主题",
  },
} as const;

const featureIcons = [Sparkles, SlidersHorizontal, Languages, HardDrive];

const BUILT_WITH_TOOLS = [
  {
    name: "Claude Code",
    href: "https://www.anthropic.com/product/claude-code",
    asset: "/img/built-with/claude-code.svg",
    slug: "claude-code",
  },
  {
    name: "Cursor",
    href: "https://cursor.com/",
    asset: "/img/built-with/cursor.svg",
    slug: "cursor",
  },
  {
    name: "Devin",
    href: "https://devin.ai/",
    asset: "/img/built-with/devin.png",
    slug: "devin",
  },
  {
    name: "Command Code",
    href: "https://commandcode.ai/",
    asset: "/img/built-with/command-code.svg",
    slug: "command-code",
  },
  {
    name: "Greptile",
    href: "https://www.greptile.com/",
    asset: "/img/built-with/greptile.svg",
    slug: "greptile",
  },
  {
    name: "GitHub Copilot",
    href: "https://github.com/features/copilot",
    asset: "/img/built-with/github-copilot.svg",
    slug: "github-copilot",
  },
  {
    name: "Kilo Code",
    href: "https://kilo.ai/",
    asset: "/img/built-with/kilo-code.svg",
    slug: "kilo-code",
  },
  {
    name: "Codex",
    href: "https://openai.com/codex",
    asset: "/img/built-with/codex.svg",
    slug: "codex",
  },
] as const;

export function LandingPage() {
  const [language, setLanguage] = useState<Language>(() => {
    const saved = localStorage.getItem("openkara-site-language");
    if (saved === "en" || saved === "zh-CN") {
      return saved;
    }
    // First visit: follow the browser language so Chinese visitors land on
    // the Chinese site instead of the English default.
    return navigator.language.toLowerCase().startsWith("zh") ? "zh-CN" : "en";
  });
  const [theme, setTheme] = useState<Theme>(() => {
    const saved = localStorage.getItem("openkara-site-theme");
    if (saved === "light" || saved === "dark") {
      return saved;
    }
    // First visit: follow the OS preference so the page matches the
    // visitor's environment instead of forcing dark.
    return window.matchMedia("(prefers-color-scheme: dark)").matches
      ? "dark"
      : "light";
  });
  const copy = COPY[language];
  const appLanguage = language;

  // Apply the theme before paint so there is no dark→light flash when the
  // OS prefers light. The inline script in index.html covers the pre-JS
  // paint; this keeps the attribute in sync after hydration. Persist the
  // language so a visitor who switches to English stays in English on
  // return visits even if their browser is set to Chinese.
  useLayoutEffect(() => {
    document.documentElement.dataset.theme = theme;
    document.documentElement.lang = language;
    localStorage.setItem("openkara-site-theme", theme);
    localStorage.setItem("openkara-site-language", language);
    document
      .querySelector('meta[name="theme-color"]')
      ?.setAttribute("content", theme === "dark" ? "#08090a" : "#f7f7f5");
  }, [language, theme]);

  return (
    <div className="landing-site">
      <header className="site-header">
        <nav className="site-nav" aria-label="Primary navigation">
          <a className="brand" href="#top" aria-label="OpenKara home">
            <img src={withBase("/img/openkara-app-icon.png")} alt="" />
            <span>OpenKara</span>
          </a>
          <div className="nav-links">
            <a href="#features">{copy.nav[0]}</a>
            <a href="#workflow">{copy.nav[1]}</a>
          </div>
          <div className="nav-actions">
            <button
              className="icon-button language-button"
              type="button"
              onClick={() => setLanguage(language === "en" ? "zh-CN" : "en")}
              aria-label={
                language === "en" ? "切换到中文" : "Switch to English"
              }
            >
              <Languages size={16} />
              <span>{language === "en" ? "中文" : "EN"}</span>
            </button>
            <button
              className="icon-button theme-button"
              type="button"
              onClick={() => setTheme(theme === "dark" ? "light" : "dark")}
              aria-label={theme === "dark" ? copy.themeLight : copy.themeDark}
              title={theme === "dark" ? copy.themeLight : copy.themeDark}
            >
              {theme === "dark" ? <Moon size={17} /> : <Sun size={17} />}
            </button>
            <a
              className="pill pill-primary pill-small"
              href="https://github.com/thedavidweng/OpenKara/releases/latest"
            >
              <span className="pill-label">{copy.download}</span>
            </a>
          </div>
        </nav>
      </header>

      <main id="top">
        <section className="hero-copy content-width">
          <h1>{copy.heroTitle}</h1>
          <p>{copy.heroBody}</p>
          <div className="hero-actions">
            <a
              className="pill pill-primary"
              href="https://github.com/thedavidweng/OpenKara/releases/latest"
            >
              <span className="pill-label">
                <Download size={16} /> {copy.primary}
              </span>
            </a>
            <a
              className="pill pill-secondary"
              href="https://youtu.be/OznVDmp9igk"
              target="_blank"
              rel="noreferrer"
            >
              <span className="pill-label">
                <Play size={15} fill="currentColor" /> {copy.secondary}
              </span>
            </a>
            <span>{copy.platform}</span>
          </div>
        </section>

        <section className="preview-section" aria-label="OpenKara preview">
          <div className="preview-stage">
            <Suspense fallback={null}>
              <AppPreview language={appLanguage} theme={theme} />
            </Suspense>
          </div>
        </section>
        <section className="built-with-section" aria-label={copy.builtWith}>
          <div className="content-width">
            <p className="built-with-label">{copy.builtWith}</p>
            <div className="built-with-logos">
              {BUILT_WITH_TOOLS.map((tool) => (
                <a
                  className={`built-with-tool built-with-tool--${tool.slug}`}
                  href={tool.href}
                  key={tool.name}
                  target="_blank"
                  rel="noreferrer"
                  aria-label={tool.name}
                >
                  <img src={withBase(tool.asset)} alt={tool.name} />
                </a>
              ))}
            </div>
            <p className="built-with-disclaimer">{copy.builtWithDisclaimer}</p>
          </div>
        </section>

        {copy.sections.map((section, sectionIndex) => {
          const sectionIds = ["features", "workflow"];
          return (
            <section
              className="feature-section content-width"
              id={sectionIds[sectionIndex]}
              key={section.title}
            >
              <div className="section-heading">
                <span>{section.eyebrow}</span>
                <h2>{section.title}</h2>
                <p>{section.body}</p>
              </div>
              <div className="feature-grid">
                {copy.cards
                  .slice(sectionIndex * 2, sectionIndex * 2 + 2)
                  .map((card, cardIndex) => {
                    const Icon = featureIcons[sectionIndex * 2 + cardIndex];
                    return (
                      <article className="feature-card" key={card[0]}>
                        <Icon size={18} />
                        <h3>{card[0]}</h3>
                        <p>{card[1]}</p>
                      </article>
                    );
                  })}
              </div>
            </section>
          );
        })}

        <section className="closing-section" aria-labelledby="closing-title">
          <h2 id="closing-title">{copy.closing}</h2>
          <div className="closing-actions">
            <a
              className="pill pill-primary"
              href="https://github.com/thedavidweng/OpenKara/releases/latest"
            >
              <span className="pill-label">
                <Download size={16} /> {copy.download}
              </span>
            </a>
            <a
              className="pill pill-secondary"
              href="https://github.com/thedavidweng/OpenKara"
              target="_blank"
              rel="noreferrer"
            >
              <span className="pill-label">
                <Code size={16} /> {copy.source}
              </span>
            </a>
          </div>
        </section>
      </main>

      <footer className="site-footer content-width">
        <a className="brand footer-brand" href="#top">
          <img src={withBase("/img/openkara-app-icon.png")} alt="" />
          <span>OpenKara</span>
        </a>
        <nav
          className="footer-column footer-product"
          aria-label={copy.footerProduct}
        >
          <h2>{copy.footerProduct}</h2>
          <a href="https://github.com/thedavidweng/OpenKara/releases/latest">
            {copy.download}
          </a>
          <a href={documentHref(language, "faq")}>{copy.footerFaq}</a>
        </nav>
        <nav
          className="footer-column footer-project"
          aria-label={copy.footerProject}
        >
          <h2>{copy.footerProject}</h2>
          <a
            href="https://github.com/thedavidweng/OpenKara"
            target="_blank"
            rel="noreferrer"
          >
            {copy.footerSourceCode}
          </a>
          <a
            href="https://github.com/thedavidweng/OpenKara/blob/main/CHANGELOG.md"
            target="_blank"
            rel="noreferrer"
          >
            {copy.footerChangelog}
          </a>
          <a
            href="https://github.com/thedavidweng/OpenKara/blob/main/LICENSE"
            target="_blank"
            rel="noreferrer"
          >
            {copy.footerLicense}
          </a>
        </nav>
        <nav
          className="footer-column footer-legal"
          aria-label={copy.footerLegal}
        >
          <h2>{copy.footerLegal}</h2>
          <a href={documentHref(language, "privacy")}>{copy.footerPrivacy}</a>
          <a href={documentHref(language, "terms")}>{copy.footerTerms}</a>
        </nav>
        <div className="footer-meta">
          <span>© {new Date().getFullYear()} OpenKara</span>
        </div>
      </footer>
    </div>
  );
}
