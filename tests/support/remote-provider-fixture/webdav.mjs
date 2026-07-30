import { getTokenFromHeader, verifyAccessToken } from "./oauth.mjs";

const files = new Map();
const locks = new Map();

function send(res, status, body, headers = {}) {
  const buf = Buffer.isBuffer(body) ? body : Buffer.from(String(body));
  res.writeHead(status, { "Content-Length": buf.length, ...headers });
  res.end(buf);
}

function collectBody(req) {
  return new Promise((resolve) => {
    const chunks = [];
    req.on("data", (chunk) => chunks.push(chunk));
    req.on("end", () => resolve(Buffer.concat(chunks)));
  });
}

function getPath(url) {
  return decodeURIComponent(url.pathname.replace(/^\/webdav\/?/, "/")) || "/";
}

function hasControl(name) {
  for (const ch of name) {
    const code = ch.charCodeAt(0);
    if (code < 32 || code === 127) return true;
  }
  return false;
}

function isMalicious(name) {
  return (
    /\.\.|[\\<>|:"*?]/.test(name) || hasControl(name) || name.startsWith("/")
  );
}

function normalizePath(url) {
  const raw = getPath(url);
  if (raw.includes("..")) return null;
  return raw;
}

function checkFault(req, res, url) {
  const fault = url.searchParams.get("fault");
  if (fault === "429") {
    send(res, 429, "Too Many Requests", { "Retry-After": "0" });
    return true;
  }
  if (fault === "500") {
    send(res, 500, "Internal Server Error");
    return true;
  }
  if (fault === "timeout") {
    return true;
  }
  if (fault === "expired-creds") {
    res.writeHead(401, { "WWW-Authenticate": 'Basic realm="test"' });
    res.end();
    return true;
  }
  return false;
}

function checkAuth(req, res) {
  const token = getTokenFromHeader(req);
  const result = verifyAccessToken(token);
  if (!result.valid) {
    res.writeHead(401, { "WWW-Authenticate": "Bearer" });
    res.end();
    return null;
  }
  return result.record;
}

function propfind(url, depth) {
  const path = normalizePath(url);
  if (!path) return null;
  const token = url.searchParams.get("sync-token") || "0";
  const since = Number(token) || 0;
  const all = [...files.entries()]
    .filter(([p, f]) => p.startsWith(path) && f.mtime > since)
    .map(([p, f]) => ({
      href: `/webdav${p}`,
      displayname: p.split("/").pop() || "root",
      size: f.content.length,
      mtime: new Date(f.mtime).toUTCString(),
      etag: f.etag,
    }));

  if (depth === "1" && !files.has(path)) {
    return all.filter(
      (entry) => entry.href.split("/").length === path.split("/").length + 1,
    );
  }
  return all;
}

function buildMultistatus(items, syncToken) {
  let xml = '<?xml version="1.0"?>\n<D:multistatus xmlns:D="DAV:">\n';
  for (const item of items) {
    xml += `  <D:response>\n    <D:href>${escapeXml(item.href)}</D:href>\n    <D:propstat>\n      <D:prop>\n        <D:displayname>${escapeXml(item.displayname)}</D:displayname>\n        <D:getcontentlength>${item.size}</D:getcontentlength>\n        <D:getlastmodified>${item.mtime}</D:getlastmodified>\n        <D:getetag>${item.etag}</D:getetag>\n      </D:prop>\n      <D:status>HTTP/1.1 200 OK</D:status>\n    </D:propstat>\n  </D:response>\n`;
  }
  xml += "</D:multistatus>";
  return { xml, syncToken };
}

function escapeXml(str) {
  return String(str).replace(
    /[<>&'"]/g,
    (c) =>
      ({
        "<": "&lt;",
        ">": "&gt;",
        "&": "&amp;",
        "'": "&apos;",
        '"': "&quot;",
      })[c],
  );
}

async function handlePropfind(req, res, url) {
  const path = normalizePath(url);
  if (!path) {
    send(res, 400, "invalid path");
    return;
  }
  const items = propfind(url, req.headers.depth);
  const syncToken = String(Date.now());
  const { xml } = buildMultistatus(items, syncToken);
  res.writeHead(207, {
    "Content-Type": "text/xml; charset=utf-8",
    "Content-Length": Buffer.byteLength(xml, "utf8"),
    "Sync-Token": syncToken,
  });
  res.end(xml);
}

async function handleGet(req, res, url) {
  const path = normalizePath(url);
  if (!path || !files.has(path)) return send(res, 404, "not found");
  const file = files.get(path);
  if (url.searchParams.get("fault") === "interrupt") {
    res.writeHead(200, {
      "Content-Length": file.content.length,
      "Content-Type": "application/octet-stream",
    });
    res.write(file.content.subarray(0, Math.floor(file.content.length / 2)));
    res.socket.destroy();
    return;
  }
  res.writeHead(200, {
    "Content-Type": "application/octet-stream",
    "Content-Length": file.content.length,
    ETag: file.etag,
  });
  res.end(file.content);
}

async function handlePut(req, res, url) {
  const path = normalizePath(url);
  if (!path) return send(res, 400, "invalid path");
  const name = path.split("/").pop();
  if (isMalicious(name)) return send(res, 400, "malicious filename");

  const clientId =
    req.headers["client-id"] || url.searchParams.get("client") || "default";
  const fault = url.searchParams.get("fault");

  if (fault === "conflict" && files.has(path)) {
    return send(res, 409, "conflict");
  }
  if (
    fault === "two-clients" &&
    locks.has(path) &&
    locks.get(path) !== clientId
  ) {
    return send(res, 423, "locked by another client");
  }

  const body = await collectBody(req);
  if (fault === "interrupt") {
    res.writeHead(201);
    res.end();
    return;
  }

  const etag = `"${Date.now().toString(36)}"`;
  files.set(path, { content: body, mtime: Date.now(), etag });
  locks.set(path, clientId);
  res.writeHead(201, { ETag: etag });
  res.end();
}

async function handleDelete(req, res, url) {
  const path = normalizePath(url);
  if (!path || !files.has(path)) return send(res, 404, "not found");
  files.delete(path);
  locks.delete(path);
  res.writeHead(204);
  res.end();
}

async function handleMkcol(req, res, url) {
  const path = normalizePath(url);
  if (!path) return send(res, 400, "invalid path");
  files.set(path, {
    content: Buffer.alloc(0),
    mtime: Date.now(),
    etag: '"collection"',
  });
  res.writeHead(201);
  res.end();
}

async function handleLock(req, res, url) {
  const path = normalizePath(url);
  const clientId =
    req.headers["client-id"] || url.searchParams.get("client") || "default";
  if (!path) return send(res, 400, "invalid path");
  locks.set(path, clientId);
  const body = JSON.stringify({ locked: path, client: clientId });
  res.writeHead(200, {
    "Content-Type": "application/json",
    "Content-Length": Buffer.byteLength(body, "utf8"),
  });
  res.end(body);
}

async function handleUnlock(req, res, url) {
  const path = normalizePath(url);
  if (!path) return send(res, 400, "invalid path");
  locks.delete(path);
  res.writeHead(204);
  res.end();
}

export default async function handleWebDAV(req, res) {
  const url = new URL(req.url, `http://${req.headers.host}`);
  if (checkFault(req, res, url)) return true;
  if (!checkAuth(req, res)) return true;

  if (isMalicious(getPath(url).split("/").pop())) {
    return send(res, 400, "malicious filename") || true;
  }

  if (req.method === "PROPFIND") return (handlePropfind(req, res, url), true);
  if (req.method === "GET" || req.method === "HEAD")
    return (handleGet(req, res, url), true);
  if (req.method === "PUT") return (handlePut(req, res, url), true);
  if (req.method === "DELETE") return (handleDelete(req, res, url), true);
  if (req.method === "MKCOL") return (handleMkcol(req, res, url), true);
  if (req.method === "LOCK") return (handleLock(req, res, url), true);
  if (req.method === "UNLOCK") return (handleUnlock(req, res, url), true);

  res.writeHead(405);
  res.end();
  return true;
}
