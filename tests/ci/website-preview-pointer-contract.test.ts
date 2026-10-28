import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { JSDOM } from "jsdom";
import { describe, expect, test } from "vite-plus/test";

const projectRoot = fileURLToPath(new URL("../..", import.meta.url));
const siteCss = readFileSync(join(projectRoot, "website/src/site.css"), "utf8");

describe("website preview pointer contract", () => {
  test("computed style keeps song-switch rows clickable and other preview buttons inert", () => {
    const { window } = new JSDOM(
      `<!doctype html>
      <html>
        <head><style>${siteCss}</style></head>
        <body>
          <div class="product-preview">
            <div data-preview-interaction-mode="playlist-only">
              <button type="button" data-preview-song-switch="true">One Last Kiss</button>
              <button type="button">Separate</button>
            </div>
          </div>
        </body>
      </html>`,
      { pretendToBeVisual: true },
    );
    const [songSwitch, blocked] = window.document.querySelectorAll("button");
    expect(window.getComputedStyle(songSwitch).pointerEvents).not.toBe("none");
    expect(window.getComputedStyle(blocked).pointerEvents).toBe("none");
  });

  test("playback slider stays clickable in the preview", () => {
    const { window } = new JSDOM(
      `<!doctype html>
      <html>
        <head><style>${siteCss}</style></head>
        <body>
          <div class="product-preview">
            <div data-preview-interaction-mode="playlist-only">
              <div role="slider" data-preview-playback-interactive="true"></div>
              <input type="range" />
            </div>
          </div>
        </body>
      </html>`,
      { pretendToBeVisual: true },
    );
    const slider = window.document.querySelector("[role='slider']");
    const blocked = window.document.querySelector("input");
    expect(slider).toBeTruthy();
    expect(blocked).toBeTruthy();
    expect(window.getComputedStyle(slider!).pointerEvents).not.toBe("none");
    expect(window.getComputedStyle(blocked!).pointerEvents).toBe("none");
  });

  test("lyrics-interactive romanize control stays clickable in the preview", () => {
    const { window } = new JSDOM(
      `<!doctype html>
      <html>
        <head><style>${siteCss}</style></head>
        <body>
          <div class="product-preview">
            <div data-preview-interaction-mode="playlist-only">
              <button type="button" data-preview-lyrics-interactive="true">Romanize</button>
              <button type="button">Separate</button>
            </div>
          </div>
        </body>
      </html>`,
      { pretendToBeVisual: true },
    );
    const [romanize, blocked] = window.document.querySelectorAll("button");
    expect(window.getComputedStyle(romanize).pointerEvents).not.toBe("none");
    expect(window.getComputedStyle(blocked).pointerEvents).toBe("none");
  });
});
