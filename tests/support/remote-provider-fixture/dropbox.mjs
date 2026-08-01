import {
  getTokenFromHeader,
  verifyAccessToken,
  checkRetryLimit,
  redactSecret,
} from "./oauth.mjs";

const files = new Map();
const cursors = new Map();
let nextId = 1;

function send(res, status, body, headers = {}) {
  const data = JSON.stringify(body);
  res.writeHead(status, {
    "Content-Type": "application/json",
    "Content-Length": Buffer.byteLength(data),
    ...headers,
  });
  res.end(data);
}

function collectBody(req) {
  return new Promise((resolve) => {
    const chunks = [];
    req.on("data", (chunk) => chunks.push(chunk));
    req.on("end", () => resolve(Buffer.concat(chunks)));
  });
}

function getArg(req, url) {
  const header = req.headers["dropbox-api-arg"];
  if (header) {
    try {
      return JSON.parse(header);
    } catch {
      return {};
    }
  }
  return {
    path: url.searchParams.get("path") || "",
    cursor: url.searchParams.get("cursor") || "",
    mode: url.searchParams.get("mode") || "add",
  };
}

function listFiles(cursor, limit) {
  const key = cursor || "0";
  const all = [...files.values()];
  const start = Number(key) || 0;
  const end = Math.min(start + limit, all.length);
  const slice = all.slice(start, end);
  return {
    entries: slice.map((f) => ({
      ".tag": "file",
      name: f.name,
      path_lower: f.path,
      size: f.content.length,
    })),
    cursor: end < all.length ? String(end) : null,
    has_more: end < all.length,
  };
}

function checkAuth(req, res) {
  const token = getTokenFromHeader(req);
  const result = verifyAccessToken(token);
  if (!result.valid) {
    res.writeHead(401);
    res.end();
    return null;
  }
  return result.record;
}

export default async function handleDropbox(req, res) {
  const url = new URL(req.url, `http://${req.headers.host}`);
  const tokenRecord = checkAuth(req, res);
  if (!tokenRecord) return true;

  const fault = url.searchParams.get("fault");
  if (fault === "timeout") return true;
  if (fault === "429") {
    res.writeHead(429, { "Retry-After": "0" });
    res.end();
    return true;
  }
  if (fault === "500") {
    res.writeHead(500);
    res.end();
    return true;
  }
  if (fault === "revoked") {
    res.writeHead(401);
    res.end();
    return true;
  }
  if (fault === "retry-limit") {
    if (!checkRetryLimit(tokenRecord.access)) {
      res.writeHead(429, { "Retry-After": "0" });
      res.end();
      return true;
    }
  }

  if (
    req.method === "POST" &&
    url.pathname === "/dropbox/2/files/list_folder"
  ) {
    const arg = JSON.parse((await collectBody(req)).toString("utf8") || "{}");
    const limit = arg.limit || 10;
    send(res, 200, listFiles("0", limit));
    return true;
  }

  if (
    req.method === "POST" &&
    url.pathname === "/dropbox/2/files/list_folder/continue"
  ) {
    const arg = JSON.parse((await collectBody(req)).toString("utf8") || "{}");
    send(res, 200, listFiles(arg.cursor, arg.limit || 10));
    return true;
  }

  if (req.method === "POST" && url.pathname === "/dropbox/2/files/upload") {
    const body = await collectBody(req);
    const arg = getArg(req, url);
    if (fault === "conflict" && files.has(arg.path)) {
      return (
        send(res, 409, { error: "conflict", error_summary: "conflict" }) || true
      );
    }
    const name = arg.path.split("/").pop() || "unnamed";
    const id = String(nextId++);
    files.set(arg.path, {
      id,
      name,
      path: arg.path,
      content: body,
      created: Date.now(),
    });
    cursors.set(id, arg.path);
    send(res, 200, {
      name,
      path_lower: arg.path,
      id,
      size: body.length,
      content_hash: redactSecret(body.toString("base64").slice(0, 8)),
    });
    return true;
  }

  if (
    (req.method === "POST" || req.method === "GET") &&
    url.pathname === "/dropbox/2/files/download"
  ) {
    const arg = getArg(req, url);
    const file = files.get(arg.path);
    if (!file) return send(res, 404, { error: "not_found" }) || true;
    if (fault === "interrupt") {
      res.writeHead(200, {
        "Content-Length": file.content.length,
        "Content-Type": "application/octet-stream",
        "dropbox-api-result": JSON.stringify({ name: file.name }),
      });
      res.write(file.content.subarray(0, Math.floor(file.content.length / 2)));
      res.socket.destroy();
      return true;
    }
    res.writeHead(200, {
      "Content-Length": file.content.length,
      "Content-Type": "application/octet-stream",
      "dropbox-api-result": JSON.stringify({
        name: file.name,
        size: file.content.length,
      }),
    });
    res.end(file.content);
    return true;
  }

  if (req.method === "POST" && url.pathname === "/dropbox/2/files/delete_v2") {
    const arg = JSON.parse((await collectBody(req)).toString("utf8") || "{}");
    files.delete(arg.path);
    res.writeHead(200);
    res.end(
      JSON.stringify({ metadata: { ".tag": "deleted", name: arg.path } }),
    );
    return true;
  }

  res.writeHead(404);
  res.end();
  return true;
}
