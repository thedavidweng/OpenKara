import AxeBuilder from "@axe-core/playwright";
import type { Page } from "@playwright/test";
import { expect, test as baseTest } from "../../fixtures/base-test";

export const WCAG_AA_TAGS = [
  "wcag2a",
  "wcag2aa",
  "wcag21a",
  "wcag21aa",
  "wcag22a",
  "wcag22aa",
];

export { expect };

export interface AccessibilityHelpers {
  setTheme(theme: "dark" | "light"): Promise<void>;
  setReducedMotion(reduce: boolean): Promise<void>;
  setForcedColors(active: boolean): Promise<void>;
  setZoom(scale: number): Promise<void>;
  startLiveRegionMonitor(): Promise<void>;
  getAnnouncements(): Promise<string[]>;
  axeCheck(): Promise<void>;
}

declare global {
  interface Window {
    __OPENKARA_A11Y__?: {
      announcements: string[];
    };
  }
}

export const test = baseTest.extend<{ a11y: AccessibilityHelpers }>({
  a11y: async ({ page }, use) => {
    await use({
      setTheme: (theme) => setTheme(page, theme),
      setReducedMotion: (reduce) => setReducedMotion(page, reduce),
      setForcedColors: (active) => setForcedColors(page, active),
      setZoom: (scale) => setZoom(page, scale),
      startLiveRegionMonitor: () => startLiveRegionMonitor(page),
      getAnnouncements: () => getAnnouncements(page),
      axeCheck: () => axeCheck(page),
    });
  },
});

async function setTheme(page: Page, theme: "dark" | "light") {
  await page.emulateMedia({ colorScheme: theme });
  await page.evaluate((selectedTheme) => {
    document.documentElement.dataset.theme = selectedTheme;
    document.documentElement.style.colorScheme = selectedTheme;
  }, theme);
}

async function setReducedMotion(page: Page, reduce: boolean) {
  await page.emulateMedia({
    reducedMotion: reduce ? "reduce" : "no-preference",
  });
}

async function setForcedColors(page: Page, active: boolean) {
  await page.emulateMedia({
    forcedColors: active ? "active" : "none",
  });
}

async function setZoom(page: Page, scale: number) {
  await page.evaluate((zoomScale) => {
    document.documentElement.dataset.a11yZoom = String(zoomScale);
    document.documentElement.style.setProperty(
      "--a11y-test-zoom",
      String(zoomScale),
    );
    const style = document.documentElement.style as unknown as {
      zoom?: string;
    };
    style.zoom = String(zoomScale);
  }, scale);
}

async function startLiveRegionMonitor(page: Page) {
  await page.evaluate(() => {
    if (window.__OPENKARA_A11Y__) {
      return;
    }

    const announcements: string[] = [];
    const liveRegionSelector = [
      "[aria-live]",
      '[role="status"]',
      '[role="alert"]',
      '[role="log"]',
      '[role="timer"]',
      '[role="marquee"]',
    ].join(", ");

    const recorder = new MutationObserver((mutations) => {
      for (const mutation of mutations) {
        const rawTarget = mutation.target;
        const target =
          rawTarget instanceof Element
            ? rawTarget
            : rawTarget.parentElement instanceof Element
              ? rawTarget.parentElement
              : null;
        if (!target) {
          continue;
        }
        const text = target.textContent?.trim();
        if (text) {
          announcements.push(text);
        }
      }
    });

    const observeElement = (element: Element) => {
      recorder.observe(element, {
        childList: true,
        subtree: true,
        characterData: true,
      });
    };

    const observeLiveRegions = (root: Element | Document) => {
      root.querySelectorAll(liveRegionSelector).forEach(observeElement);
    };

    const rootObserver = new MutationObserver((mutations) => {
      for (const mutation of mutations) {
        for (const node of Array.from(mutation.addedNodes)) {
          if (node instanceof Element) {
            if (node.matches(liveRegionSelector)) {
              observeElement(node);
            }
            observeLiveRegions(node);
          }
        }
      }
    });

    const start = () => {
      observeLiveRegions(document);
      rootObserver.observe(document.body, { childList: true, subtree: true });
      window.__OPENKARA_A11Y__ = { announcements };
    };

    if (document.body) {
      start();
    } else {
      window.addEventListener("DOMContentLoaded", start, { once: true });
    }
  });
}

async function getAnnouncements(page: Page): Promise<string[]> {
  return page.evaluate(() => {
    const buffer = window.__OPENKARA_A11Y__?.announcements;
    return buffer ? [...buffer] : [];
  });
}

async function axeCheck(page: Page) {
  const results = await new AxeBuilder({ page })
    .withTags(WCAG_AA_TAGS)
    .analyze();

  expect(
    results.violations,
    results.violations
      .map(
        (violation) =>
          `${violation.id}: ${violation.nodes.map((node) => node.target.join(" ")).join(", ")}`,
      )
      .join("\n"),
  ).toEqual([]);
}
