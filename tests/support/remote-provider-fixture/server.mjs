import http from "node:http";
import handleWebDAV from "./webdav.mjs";
import handleGoogleDrive from "./google-drive.mjs";
import handleDropbox from "./dropbox.mjs";
import handleOAuth from "./oauth.mjs";

const PORT = Number(process.env.PORT || 9877);

const server = http.createServer(async (req, res) => {
  const url = new URL(req.url, `http://${req.headers.host}`);
  if (url.pathname.startsWith("/oauth")) return handleOAuth(req, res);
  if (url.pathname.startsWith("/webdav")) return handleWebDAV(req, res);
  if (url.pathname.startsWith("/google-drive"))
    return handleGoogleDrive(req, res);
  if (url.pathname.startsWith("/dropbox")) return handleDropbox(req, res);
  res.writeHead(404);
  res.end();
});

server.listen(PORT, () =>
  console.log(`remote-provider fixture: http://localhost:${PORT}`),
);
