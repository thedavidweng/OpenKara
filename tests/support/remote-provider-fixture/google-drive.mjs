import {
  getTokenFromHeader,
  verifyAccessToken,
  checkRetryLimit,
} from "./oauth.mjs";

const files = new Map();
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

function getFault(url) {
  return url.searchParams.get("fault");
}

function listFiles(pageToken, pageSize) {
  const all = [...files.values()];
  const start = pageToken ? Number(pageToken) : 0;
  const end = Math.min(start + pageSize, all.length);
  const slice = all.slice(start, end);
  return {
    files: slice.map((f) => ({
      id: f.id,
      name: f.name,
      mimeType: f.mimeType,
      size: f.content.length,
    })),
    nextPageToken: end < all.length ? String(end) : null,
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

export default async function handleGoogleDrive(req, res) {
  const url = new URL(req.url, `http://${req.headers.host}`);
  const tokenRecord = checkAuth(req, res);
  if (!tokenRecord) return true;

  const fault = getFault(url);
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

  if (req.method === "GET" && url.pathname === "/google-drive/v3/files") {
    const pageSize = Number(url.searchParams.get("pageSize") || 10);
    const pageToken = url.searchParams.get("pageToken") || "0";
    send(res, 200, listFiles(pageToken, pageSize));
    return true;
  }

  if (
    req.method === "GET" &&
    url.pathname.startsWith("/google-drive/v3/files/")
  ) {
    const id = url.pathname.split("/").pop();
    const file = files.get(id);
    if (!file) return send(res, 404, { error: "not found" }) || true;
    if (fault === "interrupt") {
      res.writeHead(200, {
        "Content-Length": file.content.length,
        "Content-Type": "application/octet-stream",
      });
      res.write(file.content.subarray(0, Math.floor(file.content.length / 2)));
      res.socket.destroy();
      return true;
    }
    res.writeHead(200, {
      "Content-Length": file.content.length,
      "Content-Type": "application/octet-stream",
    });
    res.end(file.content);
    return true;
  }

  if (req.method === "POST" && url.pathname === "/google-drive/v3/upload") {
    const body = await collectBody(req);
    const name = url.searchParams.get("name") || "unnamed";
    if (
      fault === "conflict" &&
      [...files.values()].some((f) => f.name === name)
    ) {
      return send(res, 409, { error: "conflict" }) || true;
    }
    const id = String(nextId++);
    const metadata = url.searchParams.get("metadata");
    const mimeType = metadata
      ? JSON.parse(metadata).mimeType || "application/octet-stream"
      : "application/octet-stream";
    files.set(id, { id, name, mimeType, content: body, created: Date.now() });
    send(res, 200, { id, name, mimeType, size: body.length });
    return true;
  }

  if (
    req.method === "PATCH" &&
    url.pathname.startsWith("/google-drive/v3/files/")
  ) {
    const id = url.pathname.split("/").pop();
    if (!files.has(id)) return send(res, 404, { error: "not found" }) || true;
    const file = files.get(id);
    const updates = JSON.parse(
      (await collectBody(req)).toString("utf8") || "{}",
    );
    if (updates.name) file.name = updates.name;
    if (updates.content) file.content = Buffer.from(updates.content);
    send(res, 200, { id, name: file.name, size: file.content.length });
    return true;
  }

  if (
    req.method === "DELETE" &&
    url.pathname.startsWith("/google-drive/v3/files/")
  ) {
    const id = url.pathname.split("/").pop();
    files.delete(id);
    res.writeHead(204);
    res.end();
    return true;
  }

  res.writeHead(404);
  res.end();
  return true;
}
