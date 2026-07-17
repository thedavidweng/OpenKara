import { lazy, Suspense, useEffect, useState } from "react";
import {
  Code2,
  Download,
  HardDrive,
  Languages,
  Moon,
  Music2,
  Play,
  SlidersHorizontal,
  Sparkles,
  Sun,
} from "lucide-react";

const AppPreview = lazy(() =>
  import("./AppPreview").then((module) => ({ default: module.AppPreview })),
);

type Language = "en" | "zh-CN";
type Theme = "dark" | "light";

const COPY = {
  en: {
    nav: ["Features", "Workflow", "Local first"],
    download: "Download",
    heroTitle: "Your music, on your stage.",
    heroBody:
      "OpenKara turns the songs you already own into a complete karaoke experience—on-device stem separation, synced lyrics, and precise mixing in one open-source desktop app.",
    primary: "Download free",
    secondary: "Watch demo",
    platform: "For macOS, Windows, and Linux",
    previewLabel: "A live preview built from the OpenKara app itself",
    previewNote:
      "Choose a song, search the library, or use the playback controls. This is the current React interface running with local mock data—not a screenshot.",
    sections: [
      {
        eyebrow: "AI stem separation",
        title: "Turn every song into karaoke.",
        body: "Separate vocals, drums, bass, and other instruments on your computer. Use two stems for a quick backing track or four for complete control.",
      },
      {
        eyebrow: "One complete library",
        title: "Import once. Sing anytime.",
        body: "OpenKara keeps metadata, stems, and lyrics together in a self-contained library that can live on your computer, NAS, or USB drive.",
      },
      {
        eyebrow: "Local first",
        title: "Your music stays with you.",
        body: "Audio processing happens on your machine. No monthly subscription and no need to upload a private library to a third-party separation service.",
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
      [
        "Background jobs",
        "Keep browsing or singing while separation finishes.",
      ],
      ["Open source", "Apache 2.0 licensed, inspectable, and yours to keep."],
    ],
    closing: "Turn your music into karaoke tonight.",
    source: "View source",
    themeDark: "Switch to dark theme",
    themeLight: "Switch to light theme",
  },
  "zh-CN": {
    nav: ["功能", "体验", "本地优先"],
    download: "下载",
    heroTitle: "让你的音乐，成为你的舞台。",
    heroBody:
      "OpenKara 把你已有的歌曲变成完整的 Karaoke 体验：端侧音轨分离、同步歌词和精细混音，全部集中在一个开源桌面应用中。",
    primary: "免费下载",
    secondary: "观看演示",
    platform: "支持 macOS、Windows 和 Linux",
    previewLabel: "直接由 OpenKara 应用构建的实时预览",
    previewNote:
      "选择歌曲、搜索曲库或操作播放器。这是当前 React 界面配合本地 mock 数据运行的效果，不是截图。",
    sections: [
      {
        eyebrow: "AI 音轨分离",
        title: "把每一首歌都变成 Karaoke。",
        body: "在你的电脑上分离人声、鼓、贝斯和其他乐器。双轨模式适合快速开唱，四轨模式提供完整控制。",
      },
      {
        eyebrow: "一个完整曲库",
        title: "导入一次，随时开唱。",
        body: "OpenKara 把元数据、分轨与歌词集中在自包含曲库中，可存放在电脑、NAS 或 USB 硬盘。",
      },
      {
        eyebrow: "本地优先",
        title: "你的音乐留在你的设备上。",
        body: "音频处理在本机完成。没有按月订阅，也无需把私人曲库上传到第三方分离服务。",
      },
    ],
    cards: [
      ["2 或 4 轨", "快速生成伴奏，或分别控制每一个音轨。"],
      ["实时混音", "播放中调整各轨，无需重新处理歌曲。"],
      ["同步歌词", "支持时间歌词、内嵌歌词、LRC 与 CD+G。"],
      ["便携曲库", "一个自包含目录即可完成备份与迁移。"],
      ["后台任务", "分离在后台完成，不打断浏览与演唱。"],
      ["开源", "Apache 2.0 许可，透明、可检查、长期可用。"],
    ],
    closing: "今晚就把你的音乐变成 Karaoke。",
    source: "查看源代码",
    themeDark: "切换到深色主题",
    themeLight: "切换到浅色主题",
  },
} as const;

const featureIcons = [
  Sparkles,
  SlidersHorizontal,
  Languages,
  HardDrive,
  Music2,
  Code2,
];

export function LandingPage() {
  const [language, setLanguage] = useState<Language>(() =>
    navigator.language.toLowerCase().startsWith("zh") ? "zh-CN" : "en",
  );
  const [theme, setTheme] = useState<Theme>(() => {
    const saved = localStorage.getItem("openkara-site-theme");
    return saved === "light" ? "light" : "dark";
  });
  const copy = COPY[language];
  const appLanguage = language;

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    document.documentElement.lang = language;
    localStorage.setItem("openkara-site-theme", theme);
    document
      .querySelector('meta[name="theme-color"]')
      ?.setAttribute("content", theme === "dark" ? "#08090a" : "#f7f7f5");
  }, [language, theme]);

  return (
    <div className="landing-site">
      <header className="site-header">
        <nav className="site-nav" aria-label="Primary navigation">
          <a className="brand" href="#top" aria-label="OpenKara home">
            <img src="/img/openkara-app-icon.png" alt="" />
            <span>OpenKara</span>
          </a>
          <div className="nav-links">
            <a href="#features">{copy.nav[0]}</a>
            <a href="#workflow">{copy.nav[1]}</a>
            <a href="#local-first">{copy.nav[2]}</a>
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
              {copy.download}
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
              <Download size={16} /> {copy.primary}
            </a>
            <a
              className="pill pill-secondary"
              href="https://youtu.be/OznVDmp9igk"
              target="_blank"
              rel="noreferrer"
            >
              <Play size={15} fill="currentColor" /> {copy.secondary}
            </a>
            <span>{copy.platform}</span>
          </div>
        </section>

        <section className="preview-section" aria-labelledby="preview-title">
          <Suspense fallback={null}>
            <AppPreview language={appLanguage} />
          </Suspense>
          <div className="preview-caption content-width">
            <span id="preview-title">{copy.previewLabel}</span>
            <p>{copy.previewNote}</p>
          </div>
        </section>

        {copy.sections.map((section, sectionIndex) => {
          const sectionIds = ["features", "workflow", "local-first"];
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

        <section className="closing-section content-width">
          <div>
            <span>OpenKara</span>
            <h2>{copy.closing}</h2>
          </div>
          <a
            className="pill pill-primary"
            href="https://github.com/thedavidweng/OpenKara/releases/latest"
          >
            <Download size={16} /> {copy.download}
          </a>
        </section>
      </main>

      <footer className="site-footer content-width">
        <a className="brand" href="#top">
          <img src="/img/openkara-app-icon.png" alt="" />
          <span>OpenKara</span>
        </a>
        <span>Apache 2.0</span>
        <a href="https://github.com/thedavidweng/OpenKara">
          <Code2 size={15} /> {copy.source}
        </a>
      </footer>
    </div>
  );
}
