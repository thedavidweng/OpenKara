import { createRequire } from "node:module";
import { createInterface } from "node:readline";
import { existsSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { dispatch, operations } from "./protocol.mjs";

// Upstream logs can contain request cookies; only our protocol may write output.
for (const method of ["log", "info", "warn", "error", "debug", "trace"])
  console[method] = () => {};
const anonymousToken = join(tmpdir(), "anonymous_token");
if (!existsSync(anonymousToken)) writeFileSync(anonymousToken, "");
const require = createRequire(import.meta.url);
const request = require("@neteasecloudmusicapienhanced/api/util/request.js");
const {
  cookieToJson,
} = require("@neteasecloudmusicapienhanced/api/util/index.js");
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
for await (const line of createInterface({
  input: process.stdin,
  crlfDelay: Infinity,
})) {
  let result;
  try {
    result = await dispatch(api, JSON.parse(line));
  } catch {
    result = { error: "Invalid NetEase request" };
  }
  process.stdout.write(`${JSON.stringify(result)}\n`);
}
