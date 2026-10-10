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
  window.__openkaraCallbacks = new Set();
  console.log("OBSERVER_READY", location.href, document.hasFocus());
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

    const conditions = new Map();
    const source = `(${observe.toString()})();`;
    const send = (method, params = {}) =>
      socket.send(JSON.stringify({ id: nextId++, method, params }));
    socket.addEventListener("message", ({ data }) => {
      const message = JSON.parse(data);
      if (message.method === "Runtime.executionContextCreated") {
        const context = message.params.context;
        if (context.auxData?.isDefault && /^https?:/.test(context.origin)) {
          send("Runtime.evaluate", {
            expression: source,
            contextId: context.id,
          });
          const ipcCondition = `(() => { const m = arguments[0]; if (!m.cmd.startsWith('plugin:window|') && !m.cmd.startsWith('plugin:webview|')) return false; window.__openkaraCallbacks.add(m.callback); window.__openkaraCallbacks.add(m.error); console.log('IPC_REQUEST', JSON.stringify(m)); return false; })()`;
          const callbackCondition = `(window.__openkaraCallbacks.has(arguments[0]) && console.log('IPC_RESULT', arguments[0], JSON.stringify(arguments[1])), false)`;
          for (const [name, condition] of [
            ["postMessage", ipcCondition],
            ["runCallback", callbackCondition],
          ]) {
            conditions.set(nextId, condition);
            send("Runtime.evaluate", {
              expression: `new Promise(resolve => { const timer = setInterval(() => { const fn = window.__TAURI_INTERNALS__?.${name}; if (fn) { clearInterval(timer); resolve(fn); } }, 50); })`,
              awaitPromise: true,
              contextId: context.id,
            });
          }
          send("Runtime.evaluate", {
            expression:
              "JSON.stringify({focused:document.hasFocus(),active:document.activeElement?.outerHTML?.slice(0,250)})",
            returnByValue: true,
            contextId: context.id,
          });
        }
      }
      const condition = conditions.get(message.id);
      conditions.delete(message.id);
      if (condition && message.result?.result?.objectId) {
        send("Debugger.setBreakpointOnFunctionCall", {
          objectId: message.result.result.objectId,
          condition,
        });
        console.log("IPC_OBSERVER", target.id, "installed");
      }
      if (message.result?.result?.value)
        console.log("STATE", target.id, message.result.result.value);
      if (message.result?.exceptionDetails)
        console.error(
          "EVALUATION_ERROR",
          JSON.stringify(message.result.exceptionDetails),
        );
      if (message.method === "Runtime.consoleAPICalled")
        console.log(
          target.id,
          message.params.type,
          ...message.params.args.map((arg) => arg.value ?? arg.description),
        );
      if (message.method === "Runtime.exceptionThrown" || message.error)
        console.error(target.id, JSON.stringify(message));
    });
    await new Promise((resolve, reject) => {
      socket.addEventListener("open", resolve, { once: true });
      socket.addEventListener("error", reject, { once: true });
    });
    send("Debugger.enable");
    send("Runtime.enable");
  }
  await new Promise((resolve) => setTimeout(resolve, 500));
}
for (const socket of sockets) socket.close();
