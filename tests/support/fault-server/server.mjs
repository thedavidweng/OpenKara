import http from "node:http";
import fs from "node:fs";
import path from "node:path";
import crypto from "node:crypto";
import { FAULTS } from "./faults.mjs";

const PORT = Number(process.env.PORT || 9876);
const FIXTURE = path.join(import.meta.dirname, "fixture.bin");

const fixtureBytes = fs.readFileSync(FIXTURE);
const checksum = crypto.createHash("sha256").update(fixtureBytes).digest("hex");

function send(res, status, body, headers = {}) {
  const buf = Buffer.isBuffer(body) ? body : Buffer.from(String(body));
  res.writeHead(status, { "Content-Length": buf.length, ...headers });
  res.end(buf);
}

function json(res, status, obj) {
  const data = JSON.stringify(obj);
  send(res, status, data, { "Content-Type": "application/json" });
}

const server = http.createServer((req, res) => {
  const url = new URL(req.url, `http://${req.headers.host}`);

  if (url.pathname === "/health") {
    return send(res, 200, "ok");
  }

  if (url.pathname === "/manifest") {
    const given = url.searchParams.get("checksum");
    if (given && given !== checksum) {
      return json(res, 409, {
        error: "checksum mismatch",
        expected: checksum,
        given,
      });
    }
    return json(res, 200, {
      checksum,
      url: "/download?fault=none",
      bytes: fixtureBytes.length,
    });
  }

  if (url.pathname === "/download") {
    const fault = url.searchParams.get("fault") || "none";
    const handler = FAULTS[fault];
    if (!handler) return json(res, 404, { error: "unknown fault" });
    return handler(req, res, FIXTURE, checksum);
  }

  if (url.pathname === "/range") {
    return FAULTS.range(req, res, FIXTURE, checksum);
  }

  send(res, 404, "not found");
});

server.listen(PORT, () => {
  console.log(`fault server: http://localhost:${PORT}`);
  if (process.argv.includes("--once")) {
    server.close();
  }
});
