import { defineConfig } from "vitepress";

export default defineConfig({
  title: "OpenKara",
  description: "Turn your music library into a karaoke stage.",
  appearance: "dark",
  head: [
    [
      "link",
      {
        rel: "icon",
        type: "image/png",
        sizes: "96x96",
        href: "/img/favicon-96x96.png",
      },
    ],
    ["link", { rel: "icon", type: "image/svg+xml", href: "/img/favicon.svg" }],
    [
      "link",
      {
        rel: "apple-touch-icon",
        sizes: "180x180",
        href: "/img/apple-touch-icon.png",
      },
    ],
    ["link", { rel: "manifest", href: "/img/site.webmanifest" }],
    ["meta", { name: "apple-mobile-web-app-title", content: "OpenKara" }],
    ["meta", { name: "theme-color", content: "#08090a" }],
    ["meta", { property: "og:type", content: "website" }],
    ["meta", { property: "og:site_name", content: "OpenKara" }],
    [
      "meta",
      {
        property: "og:image",
        content: "https://openkara.103279.xyz/img/openkara-player.webp",
      },
    ],
    ["meta", { name: "twitter:card", content: "summary_large_image" }],
  ],
  locales: {
    root: {
      label: "English",
      lang: "en",
      themeConfig: {
        nav: [
          { text: "Home", link: "/" },
          { text: "FAQ", link: "/faq" },
          {
            text: "Download",
            link: "https://github.com/thedavidweng/OpenKara/releases",
          },
          {
            text: "Legal",
            items: [
              { text: "Privacy Policy", link: "/privacy" },
              { text: "Terms of Service", link: "/terms" },
            ],
          },
        ],
      },
    },
    zh: {
      label: "简体中文",
      lang: "zh-CN",
      title: "OpenKara",
      description: "把你的音乐库变成 Karaoke 舞台。",
      themeConfig: {
        nav: [
          { text: "首页", link: "/zh/" },
          { text: "常见问题", link: "/zh/faq" },
          {
            text: "下载",
            link: "https://github.com/thedavidweng/OpenKara/releases",
          },
          {
            text: "法律条款",
            items: [
              { text: "隐私政策", link: "/zh/privacy" },
              { text: "服务条款", link: "/zh/terms" },
            ],
          },
        ],
      },
    },
  },
  vite: {
    build: {
      target: "es2022",
    },
  },
  themeConfig: {
    darkModeSwitchTitle: "Switch to dark theme",
    lightModeSwitchTitle: "Switch to light theme",
    search: {
      provider: "local",
    },
    socialLinks: [
      { icon: "github", link: "https://github.com/thedavidweng/OpenKara" },
    ],
    footer: {
      message: "Licensed under Apache 2.0",
    },
    editLink: {
      pattern:
        "https://github.com/thedavidweng/OpenKara/edit/main/website/:path",
    },
  },
});
