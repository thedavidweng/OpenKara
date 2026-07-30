import fs from "node:fs";

const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

function send(res, status, body, headers = {}) {
  const buf = Buffer.isBuffer(body) ? body : Buffer.from(String(body));
  res.writeHead(status, { "Content-Length": buf.length, ...headers });
  res.end(buf);
}

function buildTar(payload, name) {
  const header = Buffer.alloc(512, 0);
  const size = payload.length;
  const mtime = Math.floor(Date.now() / 1000);

  header.write(name, 0, 100, "utf8");
  header.write("0000644 ", 100, 8, "utf8");
  header.write("0001750 ", 108, 8, "utf8");
  header.write("0001750 ", 116, 8, "utf8");
  header.write(size.toString(8).padStart(11, "0") + " ", 124, 12, "utf8");
  header.write(mtime.toString(8).padStart(11, "0") + " ", 136, 12, "utf8");
  header.write("        ", 148, 8, "utf8");
  header.write("0", 156, 1, "utf8");
  header.write("ustar\x00", 257, 6, "utf8");
  header.write("00", 263, 2, "utf8");

  let sum = 0;
  for (let i = 0; i < 512; i += 1) sum += header[i];
  header.write(sum.toString(8).padStart(6, "0") + "\0 ", 148, 8, "utf8");

  const padding = (512 - (size % 512)) % 512;
  return Buffer.concat([
    header,
    payload,
    Buffer.alloc(padding),
    Buffer.alloc(1024),
  ]);
}

function parseRange(header, total) {
  const match = header.match(/bytes=(\d*)-(\d*)/);
  if (!match) return null;
  let start = match[1] ? parseInt(match[1], 10) : 0;
  let end = match[2] ? parseInt(match[2], 10) : total - 1;
  if (match[1] === "") start = Math.max(0, total - end);
  if (match[2] === "") end = total - 1;
  if (
    Number.isNaN(start) ||
    Number.isNaN(end) ||
    start < 0 ||
    start >= total ||
    start > end
  )
    return null;
  return { start, end };
}

export const FAULTS = {
  none: (req, res, fixture) =>
    send(res, 200, fs.readFileSync(fixture), {
      "Content-Type": "application/octet-stream",
    }),

  delayed: async (req, res, fixture) => {
    const delay = Number(
      new URL(req.url, `http://${req.headers.host}`).searchParams.get(
        "delay",
      ) || 2000,
    );
    await wait(delay);
    return FAULTS.none(req, res, fixture);
  },

  dropped: (req, res, fixture) => {
    const full = fs.readFileSync(fixture);
    res.writeHead(200, {
      "Content-Length": full.length,
      "Content-Type": "application/octet-stream",
    });
    res.write(full.subarray(0, 16));
    res.socket.destroy();
  },

  partial: (req, res, fixture) => {
    const full = fs.readFileSync(fixture);
    res.writeHead(200, {
      "Content-Length": full.length,
      "Content-Type": "application/octet-stream",
    });
    res.end(full.subarray(0, Math.floor(full.length / 2)));
  },

  "incorrect-content-length": (req, res, fixture) => {
    const full = fs.readFileSync(fixture);
    res.writeHead(200, {
      "Content-Length": full.length + 1024,
      "Content-Type": "application/octet-stream",
    });
    res.end(full);
  },

  "http-429": (req, res) =>
    send(res, 429, "Too Many Requests", { "Retry-After": "1" }),

  "http-500": (req, res) => send(res, 500, "Internal Server Error"),

  "redirect-loop": (req, res) => {
    res.writeHead(302, { Location: req.url });
    res.end();
  },

  "invalid-archive": (req, res) => {
    const tar = buildTar(Buffer.from("{ not json }"), "model.json");
    send(res, 200, tar, { "Content-Type": "application/x-tar" });
  },

  "checksum-mismatch": (req, res, fixture, hash) => {
    const full = fs.readFileSync(fixture);
    full[0] ^= 0xff;
    send(res, 200, full, {
      "X-Checksum-Sha256": hash,
      "Content-Type": "application/octet-stream",
    });
  },

  range: (req, res, fixture) => {
    const full = fs.readFileSync(fixture);
    const range = req.headers.range;
    if (!range) {
      return send(res, 200, full, {
        "Accept-Ranges": "bytes",
        "Content-Type": "application/octet-stream",
      });
    }
    const parsed = parseRange(range, full.length);
    if (!parsed) {
      return send(res, 416, "Range Not Satisfiable", {
        "Content-Range": `bytes */${full.length}`,
      });
    }
    const { start, end } = parsed;
    res.writeHead(206, {
      "Content-Range": `bytes ${start}-${end}/${full.length}`,
      "Content-Length": end - start + 1,
      "Accept-Ranges": "bytes",
      "Content-Type": "application/octet-stream",
    });
    res.end(full.subarray(start, end + 1));
  },
};
