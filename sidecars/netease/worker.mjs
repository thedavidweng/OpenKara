import { createRequire } from "node:module";
import { createInterface } from "node:readline";
import { existsSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { handleLine, operations, serve } from "./protocol.mjs";

// Upstream logs can contain request cookies; only our protocol may write output.
for (const method of ["log", "info", "warn", "error", "debug", "trace"])
  console[method] = () => {};
const anonymousToken = join(tmpdir(), "anonymous_token");
if (!existsSync(anonymousToken)) writeFileSync(anonymousToken, "");
const require = createRequire(import.meta.url);
const request = require("@neteasecloudmusicapienhanced/api/util/request.js");
const {
  cookieToJson,
  generateRandomChineseIP,
} = require("@neteasecloudmusicapienhanced/api/util/index.js");
// One China Client Address per worker, like the API server's global.cnIp.
const chinaClientAddress = generateRandomChineseIP();
const api = Object.fromEntries(
  operations.map((name) => {
    const operation = require(
      `@neteasecloudmusicapienhanced/api/module/${name}.js`,
    );
    return [
      name,
      (query) =>
        operation({ ...query, cookie: cookieToJson(query.cookie) }, request),
    ];
  }),
);
await serve(
  createInterface({ input: process.stdin, crlfDelay: Infinity }),
  (line) => handleLine(api, line, chinaClientAddress),
  (response) => process.stdout.write(`${JSON.stringify(response)}\n`),
);
