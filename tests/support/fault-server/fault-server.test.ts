import { createHash } from "node:crypto";
import { createServer } from "node:http";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { afterAll, beforeAll, describe, expect, test } from "vitest";
import { FAULTS } from "./faults.mjs";

const dirname = path.dirname(fileURLToPath(import.meta.url));
const fixturePath = path.join(dirname, "fixture.bin");
const fixtureBytes = readFileSync(fixturePath);
const checksum = createHash("sha256").update(fixtureBytes).digest("hex");

function listen(handler: (req: any, res: any) => void): Promise<{
  baseUrl: string;
  close: () => Promise<void>;
}> {
  const server = createServer(handler);
  return new Promise((resolve, reject) => {
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      if (!address || typeof address === "string") {
        reject(new Error("failed to bind fault server"));
        return;
      }
      resolve({
        baseUrl: `http://127.0.0.1:${address.port}`,
        close: () =>
          new Promise((closeResolve, closeReject) => {
            server.close((error) => {
              if (error) closeReject(error);
              else closeResolve();
            });
          }),
      });
    });
  });
}

describe("fault-server delivery modes", () => {
  let baseUrl = "";
  let close: (() => Promise<void>) | undefined;

  beforeAll(async () => {
    const server = await listen((req, res) => {
      const url = new URL(req.url ?? "/", `http://${req.headers.host}`);
      if (url.pathname === "/health") {
        res.writeHead(200);
        res.end("ok");
        return;
      }
      if (url.pathname === "/download") {
        const fault = url.searchParams.get("fault") || "none";
        const handler = (FAULTS as Record<string, Function>)[fault];
        if (!handler) {
          res.writeHead(404);
          res.end(JSON.stringify({ error: "unknown fault" }));
          return;
        }
        void handler(req, res, fixturePath, checksum);
        return;
      }
      res.writeHead(404);
      res.end("not found");
    });
    baseUrl = server.baseUrl;
    close = server.close;
  });

  afterAll(async () => {
    if (close) await close();
  });

  test("serves a healthy fixture download", async () => {
    const response = await fetch(`${baseUrl}/download?fault=none`);
    expect(response.status).toBe(200);
    const body = Buffer.from(await response.arrayBuffer());
    expect(createHash("sha256").update(body).digest("hex")).toBe(checksum);
  });

  test("returns HTTP 500 for the server-error fault", async () => {
    const response = await fetch(`${baseUrl}/download?fault=http-500`);
    expect(response.status).toBe(500);
  });

  test("returns HTTP 429 for the rate-limit fault", async () => {
    const response = await fetch(`${baseUrl}/download?fault=http-429`);
    expect(response.status).toBe(429);
  });

  test("checksum mismatch fault does not yield the original digest", async () => {
    const response = await fetch(`${baseUrl}/download?fault=checksum-mismatch`);
    expect(response.status).toBe(200);
    const body = Buffer.from(await response.arrayBuffer());
    expect(createHash("sha256").update(body).digest("hex")).not.toBe(checksum);
  });

  test("dropped connection fault aborts before delivering the full body", async () => {
    await expect(
      fetch(`${baseUrl}/download?fault=dropped`).then((response) =>
        response.arrayBuffer(),
      ),
    ).rejects.toThrow();
  });

  test("invalid-archive fault returns a non-empty tar payload", async () => {
    const response = await fetch(`${baseUrl}/download?fault=invalid-archive`);
    expect(response.status).toBe(200);
    const body = Buffer.from(await response.arrayBuffer());
    expect(body.length).toBeGreaterThan(0);
    expect(createHash("sha256").update(body).digest("hex")).not.toBe(checksum);
  });
});
