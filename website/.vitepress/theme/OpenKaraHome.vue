<script setup lang="ts">
import {
  computed,
  defineComponent,
  h,
  onMounted,
  onUnmounted,
  ref,
  watch,
  type PropType,
} from "vue";
import { useData } from "vitepress";

const { isDark, lang } = useData();
const isZh = computed(() => lang.value.toLowerCase().startsWith("zh"));
const appearanceLabel = computed(() =>
  isDark.value
    ? isZh.value
      ? "切换到浅色主题"
      : "Switch to light theme"
    : isZh.value
      ? "切换到深色主题"
      : "Switch to dark theme",
);

function toggleAppearance() {
  isDark.value = !isDark.value;
}

const copy = computed(() =>
  isZh.value
    ? {
        features: "功能",
        workflow: "体验",
        local: "本地优先",
        faq: "常见问题",
        language: "English",
        languageLink: "/",
        download: "下载",
        heroTitle: "让你的音乐，成为你的舞台。",
        heroBody:
          "OpenKara 把你已有的歌曲变成完整的 Karaoke 体验：端侧 AI 人声分离、同步歌词和精细混音，全部集中在一个开源桌面应用中。",
        heroPrimary: "免费下载",
        heroSecondary: "观看演示",
        heroNote: "macOS、Windows 和 Linux",
        heroAlt: "OpenKara 播放器中的同步歌词与曲库界面",
        separation: {
          eyebrow: "AI 人声分离",
          title: "把每一首歌都变成 Karaoke。",
          body: "在你的电脑上分离人声、鼓、贝斯和其他乐器。双轨模式适合快速开唱，四轨模式带来完整控制。",
          leftTitle: "在本地分离歌曲",
          leftBody: "选择双轨或四轨模式，让 OpenKara 在后台完成处理。",
          rightTitle: "歌词始终跟上节奏",
          rightBody:
            "自动匹配时间歌词，也支持内嵌歌词、.lrc 文件与 CD+G 图形。",
          small: [
            ["2 或 4 轨", "快速生成伴奏，或分别控制人声、鼓、贝斯和其他乐器。"],
            ["实时混音", "在播放中调整每一轨，无需重新生成歌曲。"],
            ["CD+G 支持", "直接播放已有的 Karaoke 收藏与图形歌词。"],
          ],
        },
        library: {
          eyebrow: "一个完整曲库",
          title: "导入一次，随时开唱。",
          body: "拖入歌曲后，OpenKara 会处理元数据、音轨分离和歌词。曲库保持自包含，适合放在电脑、NAS 或 USB 硬盘中。",
          leftTitle: "从文件到舞台，一条完整流程",
          leftBody: "导入、处理、整理和播放都在同一个应用中完成。",
          rightTitle: "曲库跟着你走",
          rightBody: "自包含目录让备份、迁移和跨设备使用保持简单。",
          small: [
            ["后台处理", "继续浏览或播放，分离任务在后台完成。"],
            ["断点续传", "逐块检查点让中断的处理从上次位置继续。"],
            ["智能元数据", "自动读取曲名、歌手、封面和可用歌词。"],
          ],
        },
        privacy: {
          eyebrow: "本地优先",
          title: "你的音乐留在你的设备上。",
          body: "音频处理在本机完成。没有按月订阅，也无需把私人曲库上传到第三方分离服务。",
          leftTitle: "由你的电脑处理",
          leftBody: "利用本机硬件进行音轨分离，文件和结果保存在你选择的位置。",
          rightTitle: "开放、可检查、可长期使用",
          rightBody:
            "Apache 2.0 开源许可，透明的代码库，以及可以直接拥有的本地曲库。",
          platforms: [
            ["macOS", "Apple Silicon 与 Intel"],
            ["Windows", "原生安装程序与 WinGet"],
            ["Linux", "AppImage 与 Debian 软件包"],
          ],
        },
        cta: "今晚就把你的音乐变成 Karaoke。",
        ctaButton: "下载 OpenKara",
        product: "产品",
        source: "源代码",
        legal: "法律条款",
        privacyLink: "隐私政策",
        terms: "服务条款",
      }
    : {
        features: "Features",
        workflow: "Workflow",
        local: "Local first",
        faq: "FAQ",
        language: "中文",
        languageLink: "/zh/",
        download: "Download",
        heroTitle: "Your music, on your stage.",
        heroBody:
          "OpenKara turns the songs you already own into a complete karaoke experience—on-device AI stem separation, synced lyrics, and precise mixing in one open-source desktop app.",
        heroPrimary: "Download free",
        heroSecondary: "Watch demo",
        heroNote: "For macOS, Windows, and Linux",
        heroAlt: "Synced lyrics and music library in the OpenKara player",
        separation: {
          eyebrow: "AI stem separation",
          title: "Turn every song into karaoke.",
          body: "Separate vocals, drums, bass, and other instruments on your computer. Use two stems for a quick backing track or four for complete control.",
          leftTitle: "Separate songs on your device",
          leftBody:
            "Choose two or four stems and let OpenKara finish the job in the background.",
          rightTitle: "Lyrics that stay on the beat",
          rightBody:
            "Match timed lyrics automatically, or use embedded lyrics, .lrc files, and CD+G graphics.",
          small: [
            [
              "2 or 4 stems",
              "Make a quick instrumental or control vocals, drums, bass, and other tracks.",
            ],
            [
              "Live mixing",
              "Adjust every stem during playback without processing the song again.",
            ],
            [
              "CD+G included",
              "Play existing karaoke collections with their original graphics.",
            ],
          ],
        },
        library: {
          eyebrow: "One complete library",
          title: "Import once. Sing anytime.",
          body: "Drop in a song and OpenKara handles metadata, separation, and lyrics. Your library stays self-contained on your computer, NAS, or USB drive.",
          leftTitle: "One flow from file to stage",
          leftBody:
            "Import, process, organize, and play without moving between tools.",
          rightTitle: "A library that travels with you",
          rightBody:
            "A self-contained folder keeps backup, migration, and shared storage simple.",
          small: [
            [
              "Background jobs",
              "Keep browsing or singing while separation finishes behind the scenes.",
            ],
            [
              "Resumable work",
              "Per-chunk checkpoints continue interrupted processing where it stopped.",
            ],
            [
              "Smart metadata",
              "Read titles, artists, artwork, and available lyrics automatically.",
            ],
          ],
        },
        privacy: {
          eyebrow: "Local first",
          title: "Your music stays with you.",
          body: "Audio processing happens on your machine. There is no monthly subscription and no need to upload a private library to a third-party separation service.",
          leftTitle: "Powered by your computer",
          leftBody:
            "Use local hardware for stem separation while files and results stay in the location you choose.",
          rightTitle: "Open, inspectable, and yours to keep",
          rightBody:
            "Apache 2.0 licensed source, a transparent codebase, and a local library you can own for the long term.",
          platforms: [
            ["macOS", "Apple Silicon and Intel"],
            ["Windows", "Native installer and WinGet"],
            ["Linux", "AppImage and Debian package"],
          ],
        },
        cta: "Turn your music into karaoke tonight.",
        ctaButton: "Download OpenKara",
        product: "Product",
        source: "Source code",
        legal: "Legal",
        privacyLink: "Privacy policy",
        terms: "Terms of service",
      },
);

const SectionHeading = defineComponent({
  props: { eyebrow: String, title: String, body: String },
  setup: (props) => () =>
    h("div", { class: "section-heading" }, [
      h("span", props.eyebrow),
      h("h2", props.title),
      h("p", props.body),
    ]),
});

const CardCopy = defineComponent({
  props: { title: String, body: String },
  setup: (props) => () =>
    h("div", { class: "card-copy" }, [
      h("h3", props.title),
      h("p", props.body),
    ]),
});

const SmallGrid = defineComponent({
  props: {
    items: { type: Array as PropType<string[][]>, required: true },
    mode: {
      type: String as PropType<"bars" | "numbers" | "platforms">,
      required: true,
    },
  },
  setup: (props) => () =>
    h(
      "div",
      { class: "feature-grid feature-grid-small" },
      props.items.map((item, index) =>
        h("article", { class: "small-card" }, [
          h(
            "span",
            { class: "small-kicker" },
            props.mode === "platforms"
              ? "Desktop"
              : `${props.mode === "bars" ? "Audio" : "Library"} 0${index + 1}`,
          ),
          h("h3", item[0]),
          h("p", item[1]),
        ]),
      ),
    ),
});

let observer: IntersectionObserver | undefined;
const root = ref<HTMLElement>();

onMounted(() => {
  watch(
    isDark,
    (dark) => {
      document
        .querySelector('meta[name="theme-color"]')
        ?.setAttribute("content", dark ? "#08090a" : "#ffffff");
    },
    { immediate: true },
  );

  const sections = root.value?.querySelectorAll<HTMLElement>("[data-reveal]");
  if (!sections) return;
  if (!("IntersectionObserver" in window)) {
    sections.forEach((section) => section.classList.add("is-visible"));
    return;
  }

  root.value?.classList.add("reveal-ready");
  observer = new IntersectionObserver(
    (entries) => {
      entries.forEach((entry) => {
        if (entry.isIntersecting) {
          entry.target.classList.add("is-visible");
          observer?.unobserve(entry.target);
        }
      });
    },
    { rootMargin: "0px 0px -10%", threshold: 0.08 },
  );
  sections.forEach((section) => observer?.observe(section));
});

onUnmounted(() => observer?.disconnect());
</script>

<template>
  <div ref="root" class="ok-home">
    <header class="site-header">
      <nav class="site-nav" aria-label="Primary navigation">
        <a class="brand" :href="isZh ? '/zh/' : '/'" aria-label="OpenKara home">
          <img src="/img/openkara-app-icon.png" alt="" width="24" height="24" />
          <span>OpenKara</span>
        </a>
        <div class="nav-links">
          <a href="#features">{{ copy.features }}</a>
          <a href="#workflow">{{ copy.workflow }}</a>
          <a href="#local-first">{{ copy.local }}</a>
          <a :href="isZh ? '/zh/faq' : '/faq'">{{ copy.faq }}</a>
        </div>
        <div class="nav-actions">
          <a class="language-link" :href="copy.languageLink">{{
            copy.language
          }}</a>
          <button
            class="appearance-toggle"
            type="button"
            role="switch"
            :aria-label="appearanceLabel"
            :title="appearanceLabel"
            :aria-checked="isDark"
            @click="toggleAppearance"
          >
            <span class="vpi-sun appearance-sun" aria-hidden="true"></span>
            <span class="vpi-moon appearance-moon" aria-hidden="true"></span>
          </button>
          <a
            class="button button-dark button-small"
            href="https://github.com/thedavidweng/OpenKara/releases/latest"
          >
            {{ copy.download }}
          </a>
        </div>
      </nav>
    </header>

    <main>
      <section class="hero">
        <div class="hero-copy" data-reveal>
          <h1>{{ copy.heroTitle }}</h1>
          <p>{{ copy.heroBody }}</p>
          <div class="hero-actions">
            <a
              class="button button-dark"
              href="https://github.com/thedavidweng/OpenKara/releases/latest"
              >{{ copy.heroPrimary }}</a
            >
            <a
              class="button button-light"
              href="https://youtu.be/OznVDmp9igk"
              target="_blank"
              rel="noopener noreferrer"
            >
              {{ copy.heroSecondary }}
            </a>
            <span class="platform-note">{{ copy.heroNote }}</span>
          </div>
        </div>

        <div class="product-stage" data-reveal>
          <div class="stage-grid" aria-hidden="true"></div>
          <img
            class="app-screenshot"
            src="/img/openkara-player.webp"
            :alt="copy.heroAlt"
            width="1600"
            height="1048"
            fetchpriority="high"
          />
        </div>
      </section>

      <section id="features" class="feature-section" data-reveal>
        <SectionHeading
          :eyebrow="copy.separation.eyebrow"
          :title="copy.separation.title"
          :body="copy.separation.body"
        />
        <div class="feature-grid feature-grid-major">
          <article class="feature-card separation-card">
            <div class="separation-demo" aria-hidden="true">
              <div class="track-card">
                <img
                  class="track-icon"
                  src="/img/openkara-app-icon.png"
                  alt=""
                  width="38"
                  height="38"
                />
                <div>
                  <strong>Hachikō</strong><small>Fujii Kaze · 4:30</small>
                </div>
                <span class="track-status">Ready</span>
              </div>
              <div class="stem-stack">
                <div
                  v-for="(stem, index) in ['Vocals', 'Drums', 'Bass', 'Other']"
                  :key="stem"
                  class="stem-row"
                >
                  <span>{{ stem }}</span
                  ><i :style="{ '--level': `${62 + index * 6}%` }"></i
                  ><b>{{ index ? "0 dB" : "−8 dB" }}</b>
                </div>
              </div>
            </div>
            <CardCopy
              :title="copy.separation.leftTitle"
              :body="copy.separation.leftBody"
            />
          </article>

          <article class="feature-card lyrics-card">
            <div class="lyrics-demo" aria-hidden="true">
              <span>Tryin' to spread this peacefulness with y'all</span>
              <span>Our holiday's just getting started</span>
              <strong>Feel the breeze and let God bless us all</strong>
              <span>Doko ni ikō Hachikō</span>
              <div class="lyrics-progress"><i></i><b>1:10</b></div>
            </div>
            <CardCopy
              :title="copy.separation.rightTitle"
              :body="copy.separation.rightBody"
            />
          </article>
        </div>
        <SmallGrid :items="copy.separation.small" mode="bars" />
      </section>

      <section id="workflow" class="feature-section" data-reveal>
        <SectionHeading
          :eyebrow="copy.library.eyebrow"
          :title="copy.library.title"
          :body="copy.library.body"
        />
        <div class="feature-grid feature-grid-major">
          <article class="feature-card import-card">
            <div class="import-demo">
              <img src="/img/OpenKara_Import.webp" alt="" loading="lazy" />
            </div>
            <CardCopy
              :title="copy.library.leftTitle"
              :body="copy.library.leftBody"
            />
          </article>

          <article class="feature-card library-card">
            <div class="library-demo" aria-hidden="true">
              <div class="library-sidebar">
                <strong>Library</strong
                ><span class="active">All tracks <b>108</b></span
                ><span>Separated <b>99</b></span
                ><span>Playlists <b>6</b></span>
              </div>
              <div class="library-list">
                <div
                  v-for="(song, index) in [
                    ['Hachikō', 'Fujii Kaze', '4:30'],
                    ['Aria', '平沢進', '4:44'],
                    ['Kirari', 'Fujii Kaze', '3:51'],
                  ]"
                  :key="song[0]"
                >
                  <i class="cover" :class="`cover-${index + 1}`"></i
                  ><span
                    ><strong>{{ song[0] }}</strong
                    ><small>{{ song[1] }}</small></span
                  ><b>{{ song[2] }}</b>
                </div>
              </div>
            </div>
            <CardCopy
              :title="copy.library.rightTitle"
              :body="copy.library.rightBody"
            />
          </article>
        </div>
        <SmallGrid :items="copy.library.small" mode="numbers" />
      </section>

      <section id="local-first" class="feature-section" data-reveal>
        <SectionHeading
          :eyebrow="copy.privacy.eyebrow"
          :title="copy.privacy.title"
          :body="copy.privacy.body"
        />
        <div class="feature-grid feature-grid-major">
          <article class="feature-card local-card">
            <div class="local-demo" aria-hidden="true">
              <div class="device-shell">
                <span class="device-dot"></span>
                <img
                  class="device-mark"
                  src="/img/openkara-app-icon.png"
                  alt=""
                  width="58"
                  height="58"
                />
                <strong>Processing locally</strong
                ><span>Audio never leaves this device</span>
              </div>
            </div>
            <CardCopy
              :title="copy.privacy.leftTitle"
              :body="copy.privacy.leftBody"
            />
          </article>

          <article class="feature-card open-card">
            <div class="open-demo" aria-hidden="true">
              <span class="repo-label">Open source</span>
              <div>
                <small>thedavidweng / OpenKara</small
                ><strong>Open-source karaoke,<br />built in public.</strong>
              </div>
              <span class="license-pill">Apache-2.0</span>
            </div>
            <CardCopy
              :title="copy.privacy.rightTitle"
              :body="copy.privacy.rightBody"
            />
          </article>
        </div>
        <SmallGrid :items="copy.privacy.platforms" mode="platforms" />
      </section>

      <section class="closing-section" data-reveal>
        <div class="closing-card">
          <div class="closing-lines" aria-hidden="true"></div>
          <h2>{{ copy.cta }}</h2>
          <a
            class="button button-light"
            href="https://github.com/thedavidweng/OpenKara/releases/latest"
            >{{ copy.ctaButton }}</a
          >
        </div>
      </section>
    </main>

    <footer class="site-footer">
      <div class="footer-brand">
        <a class="brand" :href="isZh ? '/zh/' : '/'">
          <img
            src="/img/openkara-app-icon.png"
            alt=""
            width="24"
            height="24"
          /><span>OpenKara</span>
        </a>
        <p>Open-source desktop karaoke.</p>
      </div>
      <div class="footer-column">
        <strong>{{ copy.product }}</strong>
        <a href="https://github.com/thedavidweng/OpenKara/releases/latest">{{
          copy.download
        }}</a>
        <a :href="isZh ? '/zh/faq' : '/faq'">{{ copy.faq }}</a>
        <a href="https://github.com/thedavidweng/OpenKara">{{ copy.source }}</a>
      </div>
      <div class="footer-column">
        <strong>{{ copy.legal }}</strong>
        <a :href="isZh ? '/zh/privacy' : '/privacy'">{{ copy.privacyLink }}</a>
        <a :href="isZh ? '/zh/terms' : '/terms'">{{ copy.terms }}</a>
        <span>Apache 2.0</span>
      </div>
    </footer>
  </div>
</template>

<style>
.ok-home {
  --ink: #101010;
  --soft: #555;
  --muted: #777;
  --line: #e7e7e5;
  --paper: #fff;
  --panel: #f4f3f1;
  min-width: 320px;
  overflow: clip;
  color: var(--ink);
  background: var(--paper);
  font-family:
    Inter,
    ui-sans-serif,
    -apple-system,
    BlinkMacSystemFont,
    "Segoe UI",
    sans-serif;
  font-size: 16px;
  line-height: 1.45;
  letter-spacing: -0.015em;
}
.ok-home *,
.ok-home *:before,
.ok-home *:after {
  box-sizing: border-box;
}
.ok-home a {
  color: inherit;
  text-decoration: none;
}
.site-header {
  position: sticky;
  top: 0;
  z-index: 20;
  height: 56px;
  border-bottom: 1px solid rgba(231, 231, 229, 0.72);
  background: rgba(255, 255, 255, 0.86);
  backdrop-filter: blur(18px) saturate(1.4);
}
.site-nav {
  width: min(1180px, calc(100% - 40px));
  height: 100%;
  margin: auto;
  display: grid;
  grid-template-columns: 1fr auto 1fr;
  align-items: center;
}
.brand {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  width: max-content;
  font-size: 14px;
  font-weight: 650;
  letter-spacing: -0.035em;
}
.brand img {
  width: 24px;
  height: 24px;
  border-radius: 7px;
}
.nav-links,
.nav-actions {
  display: flex;
  align-items: center;
  gap: 24px;
  font-size: 13px;
  font-weight: 520;
}
.nav-actions {
  justify-self: end;
  gap: 12px;
}
.nav-links a,
.language-link {
  transition: color 0.16s;
}
.nav-links a:hover,
.language-link:hover {
  color: var(--muted);
}
.appearance-toggle {
  position: relative;
  width: 32px;
  height: 32px;
  flex: 0 0 32px;
  border: 1px solid var(--line);
  border-radius: 50%;
  color: var(--soft);
  background: var(--panel);
  cursor: pointer;
  transition:
    border-color 0.16s,
    color 0.16s,
    background-color 0.16s;
}
.appearance-toggle:hover {
  border-color: #b8b8b4;
  color: var(--ink);
}
.appearance-toggle:focus-visible {
  outline: 2px solid #6e8dff;
  outline-offset: 2px;
}
.appearance-toggle > span {
  position: absolute;
  inset: 7px;
  transition:
    opacity 0.16s,
    transform 0.16s;
}
.appearance-moon {
  opacity: 0;
  transform: rotate(-18deg) scale(0.75);
}
.dark .appearance-sun {
  opacity: 0;
  transform: rotate(18deg) scale(0.75);
}
.dark .appearance-moon {
  opacity: 1;
  transform: none;
}
.button {
  min-height: 38px;
  padding: 0 16px;
  border: 1px solid transparent;
  border-radius: 999px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  font-size: 14px;
  font-weight: 600;
  letter-spacing: -0.02em;
  box-shadow:
    0 1px 2px #0000001a,
    0 6px 18px #0000000d;
  transition:
    transform 0.16s,
    background-color 0.16s,
    box-shadow 0.16s;
}
.button:hover {
  transform: translateY(-1px);
  box-shadow:
    0 2px 4px #0000001a,
    0 10px 24px #00000014;
}
.button svg {
  width: 17px;
  height: 17px;
}
.button-small {
  min-height: 32px;
  padding: 0 13px;
  font-size: 13px;
}
.button-dark {
  color: #fff !important;
  background: #111;
}
.button-dark:hover {
  background: #292929;
}
.button-light {
  border-color: #00000014;
  color: #111 !important;
  background: #fffffff0;
}
.hero {
  padding: clamp(72px, 8vw, 122px) 20px 0;
}
.hero-copy,
.product-stage,
.feature-section,
.closing-section,
.site-footer {
  width: min(1180px, 100%);
  margin-inline: auto;
}
.hero-copy {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
}
.hero-copy h1 {
  max-width: 760px;
  margin: 0;
  font-size: clamp(44px, 6vw, 72px);
  font-weight: 610;
  line-height: 0.98;
  letter-spacing: -0.067em;
  text-wrap: balance;
}
.hero-copy > p {
  max-width: 680px;
  margin: 26px 0 0;
  color: var(--soft);
  font-size: clamp(17px, 1.55vw, 20px);
  line-height: 1.55;
  letter-spacing: -0.025em;
}
.hero-actions {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 9px;
  margin-top: 30px;
}
.platform-note {
  margin-left: 6px;
  color: var(--muted);
  font-size: 12px;
}
.product-stage {
  position: relative;
  isolation: isolate;
  min-height: clamp(430px, 58vw, 690px);
  margin-top: clamp(64px, 8vw, 108px);
  padding: clamp(52px, 7vw, 94px) clamp(28px, 6vw, 82px);
  display: flex;
  align-items: center;
  overflow: hidden;
  border-radius: 11px;
  background:
    radial-gradient(ellipse 75% 42% at 50% 103%, #fffffff2, #fff0 78%),
    radial-gradient(circle at 14% 64%, #e0f6ff9e, transparent 22%),
    radial-gradient(circle at 84% 60%, #e8f8ff8a, transparent 19%),
    linear-gradient(180deg, #3c9ce7, #78bceb 47%, #d9edf8);
}
.product-stage:before {
  content: "";
  position: absolute;
  inset: 28% -10% -38%;
  z-index: -1;
  background: #fff9;
  filter: blur(74px);
  border-radius: 50%;
}
.product-stage:after {
  content: "";
  position: absolute;
  inset: 0;
  z-index: 3;
  pointer-events: none;
  opacity: 0.12;
  background-image: url("data:image/svg+xml,%3Csvg viewBox='0 0 120 120' xmlns='http://www.w3.org/2000/svg'%3E%3Cfilter id='n'%3E%3CfeTurbulence type='fractalNoise' baseFrequency='.75' numOctaves='3' stitchTiles='stitch'/%3E%3C/filter%3E%3Crect width='100%25' height='100%25' filter='url(%23n)' opacity='.55'/%3E%3C/svg%3E");
  mix-blend-mode: soft-light;
}
.stage-grid {
  position: absolute;
  inset: 0;
  opacity: 0.28;
  background-image:
    linear-gradient(#ffffff8c 1px, transparent 1px),
    linear-gradient(90deg, #ffffff8c 1px, transparent 1px);
  background-size: 120px 120px;
  mask-image: linear-gradient(transparent, #000 24%, #000 70%, transparent);
}
.app-screenshot {
  position: relative;
  z-index: 2;
  display: block;
  width: 100%;
  height: auto;
}
.feature-section {
  padding: clamp(110px, 12vw, 176px) 0 0;
}
.section-heading {
  max-width: 650px;
  margin-bottom: 42px;
}
.section-heading > span {
  display: block;
  margin-bottom: 12px;
  color: #2775b7;
  font-size: 12px;
  font-weight: 680;
  letter-spacing: 0.07em;
  text-transform: uppercase;
}
.section-heading h2,
.closing-card h2 {
  margin: 0;
  font-size: clamp(34px, 4.1vw, 52px);
  font-weight: 610;
  line-height: 1.04;
  letter-spacing: -0.057em;
  text-wrap: balance;
}
.section-heading p {
  max-width: 610px;
  margin: 20px 0 0;
  color: var(--soft);
  font-size: 17px;
  line-height: 1.55;
}
.feature-grid {
  display: grid;
  gap: 16px;
}
.feature-grid-major {
  grid-template-columns: repeat(2, minmax(0, 1fr));
}
.feature-grid-small {
  grid-template-columns: repeat(3, minmax(0, 1fr));
  margin-top: 16px;
}
.feature-card,
.small-card {
  border: 1px solid var(--line);
  background: var(--panel);
}
.feature-card {
  position: relative;
  height: 590px;
  overflow: hidden;
  border-radius: 11px;
}
.feature-card:after {
  content: "";
  position: absolute;
  inset: auto 0 0;
  z-index: 2;
  height: 210px;
  background: linear-gradient(#f4f3f100, var(--panel) 34%);
  pointer-events: none;
}
.card-copy {
  position: absolute;
  inset: auto 30px 30px;
  z-index: 3;
}
.card-copy h3,
.small-card h3 {
  margin: 0;
  font-size: 17px;
  font-weight: 620;
  letter-spacing: -0.035em;
}
.card-copy p,
.small-card p {
  margin: 8px 0 0;
  color: var(--soft);
  font-size: 14px;
  line-height: 1.52;
  letter-spacing: -0.018em;
}
.small-card {
  min-height: 198px;
  padding: 24px;
  border-radius: 9px;
}
.small-kicker {
  display: block;
  margin-bottom: 34px;
  color: #7e7e7b;
  font-size: 10px;
  font-weight: 650;
  letter-spacing: 0.08em;
  text-transform: uppercase;
}
.separation-demo,
.lyrics-demo,
.library-demo,
.local-demo,
.open-demo,
.import-demo {
  position: absolute;
  inset: 0 0 145px;
}
.separation-demo {
  padding: 70px 48px 40px;
  background: radial-gradient(circle at 50% 15%, #fff, #fff0 56%);
}
.track-card {
  padding: 14px;
  display: flex;
  align-items: center;
  gap: 11px;
  border: 1px solid #dddcd9;
  border-radius: 12px;
  background: #ffffffe6;
  box-shadow: 0 12px 30px #0000000f;
}
.track-icon {
  width: 38px;
  height: 38px;
  border-radius: 8px;
}
.track-card div {
  display: flex;
  flex: 1;
  flex-direction: column;
}
.track-card strong,
.library-list strong {
  font-size: 13px;
  font-weight: 620;
}
.track-card small,
.library-list small {
  color: #898986;
  font-size: 11px;
}
.track-status {
  padding: 4px 8px;
  border-radius: 99px;
  color: #3e7451;
  background: #e8f2eb;
  font-size: 10px;
  font-weight: 650;
}
.stem-stack {
  margin: 12px 18px 0;
  padding: 18px;
  border: 1px solid #deddda;
  border-top: 0;
  border-radius: 0 0 12px 12px;
  background: #ffffffb8;
}
.stem-row {
  height: 46px;
  display: grid;
  grid-template-columns: 58px 1fr 38px;
  align-items: center;
  gap: 10px;
  border-bottom: 1px solid #e7e6e3;
  color: #565653;
  font-size: 11px;
}
.stem-row:last-child {
  border: 0;
}
.stem-row b {
  color: #999995;
  font-size: 9px;
  font-weight: 520;
  text-align: right;
}
.stem-row i {
  position: relative;
  height: 3px;
  border-radius: 3px;
  background: #dbdad7;
}
.stem-row i:before {
  content: "";
  position: absolute;
  inset: 0 auto 0 0;
  width: var(--level);
  border-radius: inherit;
  background: #222;
}
.stem-row i:after {
  content: "";
  position: absolute;
  top: 50%;
  left: var(--level);
  width: 10px;
  height: 10px;
  border: 2px solid #222;
  border-radius: 50%;
  background: #fff;
  transform: translate(-50%, -50%);
}
.lyrics-card {
  background: #111;
  border-color: #202020;
}
.lyrics-card:after {
  background: linear-gradient(#1110, #111 34%);
}
.lyrics-card .card-copy h3 {
  color: #fff;
}
.lyrics-card .card-copy p {
  color: #aaa;
}
.lyrics-demo {
  padding: 68px 56px 30px;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 25px;
  color: #434343;
  background: radial-gradient(circle at 50% 48%, #202020, #111 58%);
  font-size: 15px;
  font-weight: 590;
  text-align: center;
}
.lyrics-demo strong {
  color: #fff;
  font-size: 18px;
}
.lyrics-progress {
  position: absolute;
  inset: auto 55px 20px;
  display: flex;
  align-items: center;
  gap: 9px;
}
.lyrics-progress:before {
  content: "";
  height: 3px;
  flex: 1;
  border-radius: 2px;
  background: #323232;
}
.lyrics-progress i {
  position: absolute;
  left: 0;
  width: 34%;
  height: 3px;
  border-radius: 2px;
  background: #d6d6d6;
}
.lyrics-progress b {
  color: #727272;
  font-size: 9px;
  font-weight: 500;
}
.import-demo {
  padding: 54px 34px;
  background: linear-gradient(155deg, #dfe9f1, #fafafa 54%, #d7e6ef);
}
.import-demo img {
  width: 100%;
  height: 100%;
  object-fit: contain;
  border: 1px solid #ffffffb3;
  border-radius: 11px;
  background: #080a09;
  box-shadow: 0 20px 45px #2b43522e;
}
.library-demo {
  margin: 48px 38px 0;
  display: grid;
  grid-template-columns: 36% 1fr;
  overflow: hidden;
  border: 1px solid #dddcd9;
  border-radius: 12px;
  color: #2a2a29;
  background: #fff;
  box-shadow: 0 16px 40px #00000012;
}
.library-sidebar {
  padding: 22px 14px;
  display: flex;
  flex-direction: column;
  gap: 6px;
  border-right: 1px solid #e5e5e2;
  background: #f7f7f5;
}
.library-sidebar > strong {
  margin: 0 8px 12px;
  font-size: 12px;
}
.library-sidebar span {
  padding: 8px;
  display: flex;
  justify-content: space-between;
  border-radius: 7px;
  color: #777773;
  font-size: 10px;
}
.library-sidebar span.active {
  color: #222;
  background: #e8e8e5;
}
.library-sidebar b {
  font-weight: 500;
}
.library-list {
  padding: 16px;
}
.library-list > div {
  padding: 10px 0;
  display: flex;
  align-items: center;
  gap: 10px;
  border-bottom: 1px solid #efefed;
}
.library-list > div > span {
  display: flex;
  flex: 1;
  flex-direction: column;
}
.library-list > div > b {
  color: #a3a39f;
  font-size: 9px;
  font-weight: 500;
}
.cover {
  width: 36px;
  height: 36px;
  border-radius: 7px;
}
.cover-1 {
  background: linear-gradient(145deg, #efbc76, #a7635d 50%, #5f436c);
}
.cover-2 {
  background: linear-gradient(145deg, #1d2026, #6b7380);
}
.cover-3 {
  background: linear-gradient(145deg, #ff786c, #f2bb9c 50%, #754943);
}
.local-demo {
  display: grid;
  place-items: center;
  background: radial-gradient(circle at 50% 45%, #fff, #fff0 64%);
}
.device-shell {
  width: 250px;
  height: 280px;
  padding: 40px 28px;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  border: 1px solid #dcdcd8;
  border-radius: 26px;
  background: #fff;
  box-shadow: 0 24px 54px #00000014;
  text-align: center;
}
.device-mark {
  width: 58px;
  height: 58px;
  margin: 22px 0;
  border-radius: 15px;
}
.device-shell strong {
  font-size: 14px;
}
.device-shell > span:last-child {
  margin-top: 7px;
  color: #8a8a86;
  font-size: 10px;
}
.device-dot {
  width: 5px;
  height: 5px;
  border-radius: 50%;
  background: #58a970;
  box-shadow: 0 0 0 4px #edf6ef;
}
.open-card {
  background: #172338;
  border-color: #26364f;
}
.open-card:after {
  background: linear-gradient(#17233800, #172338 34%);
}
.open-card .card-copy h3 {
  color: #fff;
}
.open-card .card-copy p {
  color: #aebbd0;
}
.open-demo {
  padding: 88px 54px;
  color: #fff;
  background: radial-gradient(circle at 80% 15%, #478aca57, transparent 42%);
}
.repo-label {
  width: max-content;
  padding: 7px 10px;
  border: 1px solid #ffffff2e;
  border-radius: 999px;
  background: #ffffff0f;
  color: #aebbd0;
  font-size: 9px;
  font-weight: 650;
  letter-spacing: 0.08em;
  text-transform: uppercase;
}
.open-demo div {
  margin-top: 34px;
  display: flex;
  flex-direction: column;
}
.open-demo small {
  color: #91a4bf;
  font:
    10px ui-monospace,
    monospace;
}
.open-demo strong {
  margin-top: 10px;
  font-size: 25px;
  line-height: 1.15;
  letter-spacing: -0.045em;
}
.license-pill {
  position: absolute;
  right: 50px;
  bottom: 28px;
  padding: 6px 10px;
  border: 1px solid #ffffff26;
  border-radius: 99px;
  color: #aebbd0;
  font:
    9px ui-monospace,
    monospace;
}
.closing-section {
  padding-top: clamp(120px, 14vw, 210px);
}
.closing-card {
  position: relative;
  min-height: 380px;
  padding: 72px 40px;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  overflow: hidden;
  border-radius: 11px;
  color: #fff;
  background:
    radial-gradient(ellipse at 50% 130%, #ffffffe6, transparent 50%),
    linear-gradient(160deg, #1688e8, #5db5ea 58%, #caeaf7);
  text-align: center;
}
.closing-card h2 {
  position: relative;
  z-index: 2;
  max-width: 630px;
  margin-bottom: 30px;
  text-shadow: 0 1px 16px #12599124;
}
.closing-card .button {
  position: relative;
  z-index: 2;
}
.closing-lines {
  position: absolute;
  inset: 0;
  opacity: 0.3;
  background-image:
    linear-gradient(#ffffff80 1px, transparent 1px),
    linear-gradient(90deg, #ffffff80 1px, transparent 1px);
  background-size: 132px 132px;
  mask-image: radial-gradient(circle, #000, transparent 78%);
}
.site-footer {
  padding: 48px 0 70px;
  display: grid;
  grid-template-columns: 1fr 180px 180px;
  gap: 50px;
  color: #565654;
  font-size: 12px;
}
.footer-brand p {
  margin: 14px 0 0;
  color: #8c8c88;
}
.footer-column {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 8px;
}
.footer-column strong {
  margin-bottom: 7px;
  color: #222;
  font-size: 11px;
  font-weight: 650;
}
.footer-column a:hover {
  color: #111;
}
.footer-column span {
  color: #92928e;
}

/* Linear-inspired dark theme. VitePress owns and persists the `.dark` state. */
.dark .ok-home {
  --ink: #f7f8f8;
  --soft: #a1a1aa;
  --muted: #6f7178;
  --line: #232427;
  --paper: #08090a;
  --panel: #0d0e10;
  color-scheme: dark;
}
.dark .site-header {
  border-bottom-color: #202124cc;
  background: #08090ad9;
  box-shadow: 0 1px #00000066;
}
.dark .nav-links a:hover,
.dark .language-link:hover {
  color: #f2f3f5;
}
.dark .appearance-toggle {
  border-color: #2a2b2f;
  color: #d0d1d4;
  background: #111214;
}
.dark .appearance-toggle:hover {
  border-color: #45464c;
  color: #fff;
  background: #17181b;
}
.dark .button {
  box-shadow: 0 1px 2px #0009;
}
.dark .button:hover {
  box-shadow:
    0 1px 2px #000c,
    0 8px 24px #0008;
}
.dark .button-dark {
  color: #111214 !important;
  background: #f4f4f5;
}
.dark .button-dark:hover {
  background: #fff;
}
.dark .button-light {
  border-color: #2c2d31;
  color: #e5e5e7 !important;
  background: #111214e8;
}
.dark .button-light:hover {
  border-color: #424349;
  background: #17181b;
}
.dark .product-stage {
  border: 1px solid #242529;
  background:
    radial-gradient(ellipse 62% 54% at 50% 92%, #29364c66, transparent 72%),
    radial-gradient(circle at 76% 8%, #32374745, transparent 34%),
    linear-gradient(180deg, #111316, #0b0c0e 60%, #090a0c);
  box-shadow:
    inset 0 1px #ffffff08,
    0 36px 110px #0008;
}
.dark .product-stage:before {
  inset: 42% 8% -36%;
  background: #4b5f8130;
  filter: blur(90px);
}
.dark .product-stage:after {
  opacity: 0.045;
  mix-blend-mode: screen;
}
.dark .stage-grid {
  opacity: 0.07;
  background-image:
    linear-gradient(#ffffff55 1px, transparent 1px),
    linear-gradient(90deg, #ffffff55 1px, transparent 1px);
}
.dark .section-heading > span {
  color: #8a9cff;
}
.dark .feature-card,
.dark .small-card {
  border-color: #232427;
  background: #0d0e10;
  box-shadow: inset 0 1px #ffffff05;
}
.dark .feature-card:after {
  background: linear-gradient(#0d0e1000, #0d0e10 34%);
}
.dark .small-kicker {
  color: #6f7178;
}
.dark .separation-demo {
  background:
    radial-gradient(circle at 50% 8%, #262931, transparent 58%), #0d0e10;
}
.dark .track-card {
  border-color: #303137;
  background: #17181be8;
  box-shadow: 0 18px 44px #0009;
}
.dark .track-card small,
.dark .library-list small {
  color: #74767d;
}
.dark .track-status {
  color: #9dc9aa;
  background: #183021;
}
.dark .stem-stack {
  border-color: #292a2e;
  background: #111214d9;
}
.dark .stem-row {
  border-bottom-color: #28292d;
  color: #a4a5aa;
}
.dark .stem-row b {
  color: #65676d;
}
.dark .stem-row i {
  background: #303137;
}
.dark .stem-row i:before {
  background: #b9bbc0;
}
.dark .stem-row i:after {
  border-color: #d4d5d8;
  background: #111214;
}
.dark .lyrics-card {
  border-color: #292a2e;
  background: #0a0b0c;
}
.dark .lyrics-card:after {
  background: linear-gradient(#0a0b0c00, #0a0b0c 34%);
}
.dark .lyrics-demo {
  color: #414248;
  background: radial-gradient(circle at 50% 44%, #202227, #0a0b0c 62%);
}
.dark .import-demo {
  background:
    radial-gradient(circle at 25% 20%, #2b304066, transparent 48%), #0b0c0e;
}
.dark .import-demo img {
  border-color: #303238;
  box-shadow: 0 24px 60px #000b;
}
.dark .library-demo {
  border-color: #2b2c31;
  color: #e5e5e7;
  background: #121315;
  box-shadow: 0 22px 56px #000a;
}
.dark .library-sidebar {
  border-right-color: #292a2e;
  background: #0e0f11;
}
.dark .library-sidebar span {
  color: #73757c;
}
.dark .library-sidebar span.active {
  color: #e8e9eb;
  background: #202125;
}
.dark .library-list > div {
  border-bottom-color: #25262a;
}
.dark .library-list > div > b {
  color: #606269;
}
.dark .local-demo {
  background: radial-gradient(circle at 50% 42%, #24262d, transparent 64%);
}
.dark .device-shell {
  border-color: #303137;
  background: #141517;
  box-shadow:
    0 26px 64px #000a,
    inset 0 1px #ffffff08;
}
.dark .device-shell > span:last-child {
  color: #777980;
}
.dark .device-dot {
  background: #77c48b;
  box-shadow: 0 0 0 4px #1b3021;
}
.dark .open-card {
  border-color: #282a30;
  background: #0b0d11;
}
.dark .open-card:after {
  background: linear-gradient(#0b0d1100, #0b0d11 34%);
}
.dark .open-demo {
  background:
    radial-gradient(circle at 78% 18%, #384a6a55, transparent 45%),
    linear-gradient(145deg, #10141c, #0b0d11 62%);
}
.dark .repo-label {
  border-color: #40444d;
  color: #9da4b2;
  background: #ffffff08;
}
.dark .closing-card {
  border: 1px solid #28292d;
  color: #f7f8f8;
  background:
    radial-gradient(ellipse at 50% 128%, #27334a80, transparent 54%),
    linear-gradient(180deg, #101114, #0b0c0e);
  box-shadow: inset 0 1px #ffffff08;
}
.dark .closing-card h2 {
  text-shadow: none;
}
.dark .closing-lines {
  opacity: 0.06;
}
.dark .site-footer {
  color: #777980;
}
.dark .footer-brand p,
.dark .footer-column span {
  color: #5f6167;
}
.dark .footer-column strong {
  color: #cfd0d3;
}
.dark .footer-column a:hover {
  color: #f7f8f8;
}
.reveal-ready [data-reveal] {
  opacity: 0;
  transform: translateY(14px);
  transition:
    opacity 0.7s cubic-bezier(0.22, 1, 0.36, 1),
    transform 0.7s cubic-bezier(0.22, 1, 0.36, 1);
}
.reveal-ready [data-reveal].is-visible {
  opacity: 1;
  transform: none;
}
@media (prefers-reduced-motion: reduce) {
  .reveal-ready [data-reveal] {
    opacity: 1;
    transform: none;
    transition: none;
  }
  .button {
    transition: none;
  }
}
@media (max-width: 820px) {
  .site-nav {
    grid-template-columns: 1fr auto;
  }
  .nav-links {
    display: none;
  }
  .language-link {
    display: none;
  }
  .hero {
    padding-top: 66px;
  }
  .product-stage {
    min-height: 350px;
    padding: 35px 22px;
  }
  .feature-grid-major {
    grid-template-columns: 1fr;
  }
  .feature-card {
    height: 560px;
  }
  .feature-grid-small {
    grid-template-columns: 1fr;
  }
  .small-card {
    min-height: 154px;
  }
  .small-kicker {
    margin-bottom: 24px;
  }
  .site-footer {
    grid-template-columns: 1fr 1fr;
  }
  .footer-brand {
    grid-column: 1/-1;
  }
}
@media (max-width: 520px) {
  .site-header {
    height: 52px;
  }
  .site-nav {
    width: calc(100% - 28px);
  }
  .brand {
    font-size: 13px;
  }
  .brand img {
    width: 22px;
    height: 22px;
  }
  .hero {
    padding: 52px 14px 0;
  }
  .hero-copy h1 {
    font-size: 43px;
  }
  .hero-copy > p {
    margin-top: 20px;
    font-size: 16px;
  }
  .hero-actions {
    margin-top: 24px;
  }
  .platform-note {
    width: 100%;
    margin: 5px 0 0 2px;
  }
  .product-stage {
    min-height: 265px;
    margin-top: 52px;
    padding: 25px 13px;
    border-radius: 8px;
  }
  .feature-section,
  .closing-section,
  .site-footer {
    width: calc(100% - 28px);
  }
  .feature-section {
    padding-top: 94px;
  }
  .section-heading {
    margin-bottom: 28px;
  }
  .section-heading h2 {
    font-size: 36px;
  }
  .section-heading p {
    font-size: 15px;
  }
  .feature-card {
    height: 500px;
  }
  .card-copy {
    inset: auto 22px 24px;
  }
  .separation-demo {
    padding: 55px 22px 30px;
  }
  .lyrics-demo {
    padding: 58px 28px 25px;
    font-size: 13px;
  }
  .lyrics-demo strong {
    font-size: 16px;
  }
  .import-demo {
    padding: 42px 18px;
  }
  .library-demo {
    margin: 38px 18px 0;
    grid-template-columns: 40% 1fr;
  }
  .library-list {
    padding: 10px;
  }
  .library-list > div {
    gap: 7px;
  }
  .cover {
    width: 29px;
    height: 29px;
  }
  .device-shell {
    width: 220px;
    height: 250px;
  }
  .open-demo {
    padding: 70px 34px;
  }
  .open-demo strong {
    font-size: 22px;
  }
  .closing-section {
    padding-top: 110px;
  }
  .closing-card {
    min-height: 330px;
    padding: 52px 24px;
  }
  .site-footer {
    grid-template-columns: 1fr 1fr;
    gap: 40px 24px;
    padding-bottom: 50px;
  }
}
</style>
