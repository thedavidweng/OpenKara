// SPDX-License-Identifier: AGPL-3.0-only
import { createServer } from "node:https";
import { readFile } from "node:fs/promises";
import { deflateSync } from "node:zlib";

const [keyPath, certPath] = process.argv.slice(2);
if (!keyPath || !certPath)
  throw new Error("Usage: node media-server.mjs TLS_KEY TLS_CERT");
function tone(frequency) {
  const audio = Buffer.alloc(16044);
  audio.write("RIFF", 0);
  audio.writeUInt32LE(16036, 4);
  audio.write("WAVEfmt ", 8);
  audio.writeUInt32LE(16, 16);
  audio.writeUInt16LE(1, 20);
  audio.writeUInt16LE(1, 22);
  audio.writeUInt32LE(8000, 24);
  audio.writeUInt32LE(16000, 28);
  audio.writeUInt16LE(2, 32);
  audio.writeUInt16LE(16, 34);
  audio.write("data", 36);
  audio.writeUInt32LE(16000, 40);
  for (let sample = 0; sample < 8000; sample++) {
    audio.writeInt16LE(
      Math.round(4000 * Math.sin((2 * Math.PI * frequency * sample) / 8000)),
      44 + sample * 2,
    );
  }
  return audio;
}
function pngChunk(type, data) {
  const name = Buffer.from(type);
  const body = Buffer.concat([name, data]);
  let crc = 0xffffffff;
  for (const byte of body) {
    crc ^= byte;
    for (let bit = 0; bit < 8; bit++)
      crc = (crc >>> 1) ^ (crc & 1 ? 0xedb88320 : 0);
  }
  const length = Buffer.alloc(4);
  length.writeUInt32BE(data.length);
  const checksum = Buffer.alloc(4);
  checksum.writeUInt32BE((crc ^ 0xffffffff) >>> 0);
  return Buffer.concat([length, body, checksum]);
}
const header = Buffer.alloc(13);
header.writeUInt32BE(32, 0);
header.writeUInt32BE(32, 4);
header[8] = 8;
header[9] = 2;
const pixels = Buffer.alloc(32 * 97);
for (let y = 0; y < 32; y++) {
  for (let x = 0; x < 32; x++) {
    const index = y * 97 + 1 + x * 3;
    pixels[index] = 48;
    pixels[index + 1] = 96 + y * 3;
    pixels[index + 2] = 128 + x * 3;
  }
}
const artwork = Buffer.concat([
  Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
  pngChunk("IHDR", header),
  pngChunk("IDAT", deflateSync(pixels)),
  pngChunk("IEND", Buffer.alloc(0)),
]);
const assets = new Map([
  ["/tone-1.wav", { bytes: tone(440), type: "audio/wav" }],
  ["/tone-3.wav", { bytes: tone(660), type: "audio/wav" }],
  ["/artwork.png", { bytes: artwork, type: "image/png" }],
]);
const server = createServer(
  { key: await readFile(keyPath), cert: await readFile(certPath) },
  (request, response) => {
    const asset = assets.get(request.url);
    if (!asset || !["GET", "HEAD"].includes(request.method)) {
      response.writeHead(asset ? 405 : 404).end();
      return;
    }
    let start = 0;
    let end = asset.bytes.length - 1;
    let status = 200;
    const headers = { "Content-Type": asset.type, "Accept-Ranges": "bytes" };
    if (request.headers.range) {
      const range = /^bytes=(\d+)-(\d*)$/.exec(request.headers.range);
      start = range ? Number(range[1]) : -1;
      end = range?.[2] ? Math.min(Number(range[2]), end) : end;
      if (start < 0 || start > end || start >= asset.bytes.length) {
        response
          .writeHead(416, { "Content-Range": `bytes */${asset.bytes.length}` })
          .end();
        return;
      }
      status = 206;
      headers["Content-Range"] = `bytes ${start}-${end}/${asset.bytes.length}`;
    }
    headers["Content-Length"] = end - start + 1;
    response.writeHead(status, headers);
    response.end(
      request.method === "HEAD"
        ? undefined
        : asset.bytes.subarray(start, end + 1),
    );
  },
);
server.listen(9443, "127.0.0.1", () => {
  console.log(
    "Fixture HTTPS media ready on 127.0.0.1:9443 (test harness only)",
  );
});
