import { chromium } from "@playwright/test";

const endpoint = "http://127.0.0.1:9222";
let ready = false;
for (let attempt = 0; attempt < 120; attempt++) {
  try {
    const response = await fetch(`${endpoint}/json/version`);
    ready = response.ok;
  } catch {}
  if (ready) break;
  await new Promise((resolve) => setTimeout(resolve, 500));
}
if (!ready) throw new Error("WebView2 diagnostic endpoint did not start");
const browser = await chromium.connectOverCDP(endpoint);
function observe() {
  window.addEventListener(
    "keydown",
    (event) => {
      console.log(
        "INPUT_DIAGNOSTIC",
        JSON.stringify({
          key: event.key,
          code: event.code,
          ctrl: event.ctrlKey,
          alt: event.altKey,
          shift: event.shiftKey,
          meta: event.metaKey,
          trusted: event.isTrusted,
          focused: document.hasFocus(),
          target: event.target?.outerHTML?.slice(0, 250),
        }),
      );
    },
    true,
  );
  const internals = window.__TAURI_INTERNALS__;
  if (internals) {
    const invoke = internals.invoke.bind(internals);
    internals.invoke = async (command, args, options) => {
      const record = /plugin:(window|webview)\|/.test(command);
      if (record) console.log("IPC_DIAGNOSTIC", command, JSON.stringify(args));
      try {
        const result = await invoke(command, args, options);
        if (record) console.log("IPC_RESULT", command, JSON.stringify(result));
        return result;
      } catch (error) {
        if (record) console.error("IPC_ERROR", command, String(error));
        throw error;
      }
    };
  }
}
async function attach(page) {
  console.log("PAGE", page.url());
  page.on("console", (message) => console.log(message.type(), message.text()));
  page.on("pageerror", (error) => console.error("PAGE_ERROR", error.message));
  await page.addInitScript(observe);
  await page.evaluate(observe);
  console.log(
    "FOCUS",
    await page.evaluate(() => ({
      focused: document.hasFocus(),
      active: document.activeElement?.outerHTML?.slice(0, 250),
    })),
  );
}
for (const context of browser.contexts()) {
  context.on("page", (page) => attach(page).catch(console.error));
  for (const page of context.pages()) await attach(page);
}
await new Promise((resolve) => browser.on("disconnected", resolve));
