import { createServer } from "node:http";
import { afterAll, beforeAll, describe, expect, test } from "vitest";
import handleWebDAV from "./webdav.mjs";
import handleGoogleDrive from "./google-drive.mjs";
import handleDropbox from "./dropbox.mjs";
import handleOAuth from "./oauth.mjs";

function listen(handler: (req: any, res: any) => void): Promise<{
  baseUrl: string;
  close: () => Promise<void>;
}> {
  const server = createServer(handler);
  return new Promise((resolve, reject) => {
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      if (!address || typeof address === "string") {
        reject(new Error("failed to bind remote-provider fixture"));
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

describe("remote-provider deterministic fixtures", () => {
  let baseUrl = "";
  let close: (() => Promise<void>) | undefined;

  beforeAll(async () => {
    const server = await listen(async (req, res) => {
      const url = new URL(req.url ?? "/", `http://${req.headers.host}`);
      if (url.pathname.startsWith("/oauth")) return handleOAuth(req, res);
      if (url.pathname.startsWith("/webdav")) return handleWebDAV(req, res);
      if (url.pathname.startsWith("/google-drive"))
        return handleGoogleDrive(req, res);
      if (url.pathname.startsWith("/dropbox")) return handleDropbox(req, res);
      res.writeHead(404);
      res.end();
    });
    baseUrl = server.baseUrl;
    close = server.close;
  });

  afterAll(async () => {
    if (close) await close();
  });

  test("WebDAV rejects missing credentials", async () => {
    const response = await fetch(`${baseUrl}/webdav/`);
    expect(response.status).toBe(401);
  });

  test("OAuth rejects invalid state", async () => {
    const response = await fetch(
      `${baseUrl}/oauth/callback?code=demo&state=invalid`,
    );
    expect(response.status).toBeGreaterThanOrEqual(400);
  });

  test("Google Drive and Dropbox roots respond without crashing", async () => {
    const drive = await fetch(`${baseUrl}/google-drive/files`);
    const dropbox = await fetch(`${baseUrl}/dropbox/2/files/list_folder`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ path: "" }),
    });
    expect(drive.status).toBeLessThan(600);
    expect(dropbox.status).toBeLessThan(600);
  });

  test("fault query parameters return 429/500 for provider paths", async () => {
    const rateLimited = await fetch(`${baseUrl}/webdav/?fault=429`);
    const serverError = await fetch(`${baseUrl}/webdav/?fault=500`);
    expect(rateLimited.status).toBe(429);
    expect(serverError.status).toBe(500);
  });
});
