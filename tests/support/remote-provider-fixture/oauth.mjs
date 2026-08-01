import crypto from "node:crypto";

const codes = new Map();
const accessTokens = new Map();
const refreshTokens = new Map();
const revoked = new Set();
const retryCounters = new Map();

const TTL_MS = 60_000;

function now() {
  return Date.now();
}

function send(res, status, body, headers = {}) {
  const data = JSON.stringify(body);
  res.writeHead(status, {
    "Content-Type": "application/json",
    "Content-Length": Buffer.byteLength(data),
    ...headers,
  });
  res.end(data);
}

function redact(value) {
  if (!value) return "<empty>";
  if (value.length <= 8) return "*".repeat(value.length);
  return value.slice(0, 4) + "..." + value.slice(-4);
}

export function isLocalhostLoopback(redirectUri) {
  try {
    const url = new URL(redirectUri);
    return url.hostname === "localhost" && url.protocol === "http:";
  } catch {
    return false;
  }
}

export function issueAuthCode(clientId, redirectUri, state, scope = "") {
  const code = crypto.randomUUID();
  codes.set(code, { clientId, redirectUri, state, scope, created: now() });
  return code;
}

export function exchangeCode(code) {
  const record = codes.get(code);
  if (!record) return null;
  if (now() - record.created > TTL_MS) {
    codes.delete(code);
    return null;
  }
  codes.delete(code);
  return createTokens(record.clientId, record.scope);
}

export function createTokens(clientId, scope) {
  const access = crypto.randomUUID();
  const refresh = crypto.randomUUID();
  const record = {
    clientId,
    scope,
    access,
    refresh,
    created: now(),
    attempts: 0,
  };
  accessTokens.set(access, record);
  refreshTokens.set(refresh, record);
  return {
    access_token: access,
    refresh_token: refresh,
    token_type: "Bearer",
    expires_in: 3600,
  };
}

export function refreshAccessToken(refresh) {
  const record = refreshTokens.get(refresh);
  if (!record || revoked.has(refresh) || revoked.has(record.access))
    return null;
  accessTokens.delete(record.access);
  const newAccess = crypto.randomUUID();
  record.access = newAccess;
  record.created = now();
  record.attempts = 0;
  accessTokens.set(newAccess, record);
  return { access_token: newAccess, token_type: "Bearer", expires_in: 3600 };
}

export function revokeToken(token) {
  revoked.add(token);
  const record = accessTokens.get(token) || refreshTokens.get(token);
  if (record) {
    revoked.add(record.access);
    revoked.add(record.refresh);
  }
}

export function getTokenRecord(token) {
  return accessTokens.get(token) || null;
}

export function verifyAccessToken(token) {
  if (revoked.has(token)) return { valid: false, reason: "revoked" };
  const record = accessTokens.get(token);
  if (!record) return { valid: false, reason: "invalid" };
  if (now() - record.created > TTL_MS)
    return { valid: false, reason: "expired" };
  return { valid: true, record };
}

export function redactSecret(value) {
  return redact(value);
}

export function checkRetryLimit(token, limit = 3) {
  const key = token || "anonymous";
  const current = (retryCounters.get(key) || 0) + 1;
  retryCounters.set(key, current);
  return current >= limit;
}

export function getTokenFromHeader(req) {
  const auth = req.headers.authorization || "";
  if (auth.startsWith("Bearer ")) return auth.slice(7);
  const query = new URL(req.url, `http://${req.headers.host}`).searchParams;
  return query.get("access_token") || query.get("token") || "";
}

function parseBody(req) {
  return new Promise((resolve) => {
    const chunks = [];
    req.on("data", (chunk) => chunks.push(chunk));
    req.on("end", () => resolve(Buffer.concat(chunks).toString("utf8")));
  });
}

function parseForm(body) {
  const result = {};
  for (const pair of body.split("&")) {
    const [key, value] = pair.split("=");
    if (key)
      result[decodeURIComponent(key)] = value ? decodeURIComponent(value) : "";
  }
  return result;
}

async function handleAuthorize(req, res, url) {
  if (url.searchParams.get("fault") === "timeout") {
    return true;
  }
  const clientId = url.searchParams.get("client_id");
  const redirectUri = url.searchParams.get("redirect_uri");
  const state = url.searchParams.get("state");
  const responseType = url.searchParams.get("response_type");

  if (!clientId || !redirectUri || !state || responseType !== "code") {
    res.writeHead(400);
    res.end();
    return true;
  }
  if (!isLocalhostLoopback(redirectUri)) {
    res.writeHead(400);
    res.end();
    return true;
  }

  const code = issueAuthCode(clientId, redirectUri, state);
  const target = new URL(redirectUri);
  target.searchParams.set("code", code);
  target.searchParams.set("state", state);
  res.writeHead(302, { Location: target.toString() });
  res.end();
  return true;
}

async function handleToken(req, res) {
  const body = await parseBody(req);
  const form = parseForm(body);
  const url = new URL(req.url, `http://${req.headers.host}`);

  if (url.searchParams.get("fault") === "timeout") {
    return true;
  }

  if (form.grant_type === "authorization_code") {
    const tokens = exchangeCode(form.code);
    if (!tokens) {
      send(res, 400, { error: "invalid_grant" });
      return true;
    }
    send(res, 200, tokens);
    return true;
  }

  if (form.grant_type === "refresh_token") {
    const refreshed = refreshAccessToken(form.refresh_token);
    if (!refreshed) {
      send(res, 401, { error: "invalid_grant" });
      return true;
    }
    send(res, 200, refreshed);
    return true;
  }

  send(res, 400, { error: "unsupported_grant_type" });
  return true;
}

async function handleRevoke(req, res) {
  const body = await parseBody(req);
  const form = parseForm(body);
  const token = form.token;
  if (token) revokeToken(token);
  send(res, 200, { revoked: redact(token), count: revoked.size });
  return true;
}

async function handleCheck(req, res, url) {
  const token = url.searchParams.get("access_token") || "";
  const result = verifyAccessToken(token);
  send(res, result.valid ? 200 : 401, {
    valid: result.valid,
    reason: result.reason,
    token: redact(token),
    client: result.record?.clientId,
  });
  return true;
}

export default async function handleOAuth(req, res) {
  const url = new URL(req.url, `http://${req.headers.host}`);
  if (!url.pathname.startsWith("/oauth")) return false;

  if (req.method === "GET" && url.pathname === "/oauth/authorize")
    return handleAuthorize(req, res, url);
  if (req.method === "POST" && url.pathname === "/oauth/token")
    return handleToken(req, res);
  if (req.method === "POST" && url.pathname === "/oauth/revoke")
    return handleRevoke(req, res);
  if (req.method === "GET" && url.pathname === "/oauth/check")
    return handleCheck(req, res, url);

  res.writeHead(404);
  res.end();
  return true;
}
