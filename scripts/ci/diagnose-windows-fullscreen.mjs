const endpoint = "http://127.0.0.1:9222";
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
  const timer = setInterval(() => {
    const internals = window.__TAURI_INTERNALS__;
    if (!internals) return;
    clearInterval(timer);
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
  }, 50);
}

const observed = new Set();
const sockets = [];
let nextId = 1;
for (let attempt = 0; attempt < 1200; attempt++) {
  let targets;
  try {
    const response = await fetch(`${endpoint}/json/list`);
    if (!response.ok) throw new Error(`CDP target list: ${response.status}`);
    targets = await response.json();
  } catch (error) {
    if (observed.size) {
      console.log("BROWSER_CLOSED", String(error));
      break;
    }
    if (attempt === 119)
      throw new Error("WebView2 diagnostic endpoint did not start");
    await new Promise((resolve) => setTimeout(resolve, 500));
    continue;
  }
  for (const target of targets.filter((target) => target.type === "page")) {
    if (observed.has(target.id)) continue;
    observed.add(target.id);
    console.log("PAGE", target.id, target.url);
    const socket = new WebSocket(target.webSocketDebuggerUrl);
    sockets.push(socket);
    socket.addEventListener("message", ({ data }) => {
      const message = JSON.parse(data);
      if (message.method === "Runtime.consoleAPICalled") {
        console.log(
          target.id,
          message.params.type,
          ...message.params.args.map((arg) => arg.value ?? arg.description),
        );
      }
      if (message.method === "Runtime.exceptionThrown" || message.error) {
        console.error(target.id, JSON.stringify(message));
      }
    });
    await new Promise((resolve, reject) => {
      socket.addEventListener("open", resolve, { once: true });
      socket.addEventListener("error", reject, { once: true });
    });
    const send = (method, params = {}) =>
      socket.send(JSON.stringify({ id: nextId++, method, params }));
    send("Runtime.enable");
    const source = `(${observe.toString()})();`;
    send("Page.addScriptToEvaluateOnNewDocument", { source });
    send("Runtime.evaluate", { expression: source });
    send("Runtime.evaluate", {
      expression:
        "JSON.stringify({focused:document.hasFocus(),active:document.activeElement?.outerHTML?.slice(0,250)})",
      returnByValue: true,
    });
  }
  await new Promise((resolve) => setTimeout(resolve, 500));
}
for (const socket of sockets) socket.close();
