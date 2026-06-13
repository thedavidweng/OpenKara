import { execFileSync } from "node:child_process";
import { cp, mkdir, rm } from "node:fs/promises";
import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";
import zlib from "node:zlib";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const projectRoot = path.resolve(__dirname, "..");
const iconsDir = path.join(projectRoot, "src-tauri", "icons");
const iconComposerDir = path.join(iconsDir, "OpenKara.icon");
const stagingDir = path.join(iconsDir, ".liquid-glass-staging");

if (process.platform !== "darwin") {
  console.log("Skipping macOS Liquid Glass icon compile on non-darwin host");
  process.exit(0);
}

function crc32(buffer) {
  let crc = 0xffffffff;
  for (const byte of buffer) {
    crc ^= byte;
    for (let bit = 0; bit < 8; bit++) {
      crc = (crc >>> 1) ^ (0xedb88320 & -(crc & 1));
    }
  }
  return (crc ^ 0xffffffff) >>> 0;
}

function makePngChunk(type, data) {
  const typeBuffer = Buffer.from(type, "ascii");
  const chunk = Buffer.alloc(12 + data.length);
  chunk.writeUInt32BE(data.length, 0);
  typeBuffer.copy(chunk, 4);
  data.copy(chunk, 8);
  chunk.writeUInt32BE(
    crc32(Buffer.concat([typeBuffer, data])),
    8 + data.length,
  );
  return chunk;
}

function pngPaeth(left, up, upLeft) {
  const estimate = left + up - upLeft;
  const leftDistance = Math.abs(estimate - left);
  const upDistance = Math.abs(estimate - up);
  const upLeftDistance = Math.abs(estimate - upLeft);
  if (leftDistance <= upDistance && leftDistance <= upLeftDistance) return left;
  if (upDistance <= upLeftDistance) return up;
  return upLeft;
}

function writeMicLayerFromMasterIcon(inputPath, outputPath) {
  const source = readFileSync(inputPath);
  if (source.toString("hex", 0, 8) !== "89504e470d0a1a0a") {
    throw new Error(`expected PNG master icon at ${inputPath}`);
  }

  let offset = 8;
  let width = 0;
  let height = 0;
  let colorType = 0;
  const idatChunks = [];
  const preservedChunks = [];

  while (offset < source.length) {
    const length = source.readUInt32BE(offset);
    const type = source.toString("ascii", offset + 4, offset + 8);
    const dataStart = offset + 8;
    const dataEnd = dataStart + length;
    const data = source.subarray(dataStart, dataEnd);

    if (type === "IHDR") {
      width = data.readUInt32BE(0);
      height = data.readUInt32BE(4);
      colorType = data[9];
      if (
        data[8] !== 8 ||
        colorType !== 6 ||
        data[10] !== 0 ||
        data[11] !== 0 ||
        data[12] !== 0
      ) {
        throw new Error("expected an 8-bit RGBA master icon PNG");
      }
    } else if (type === "IDAT") {
      idatChunks.push(data);
    } else if (["gAMA", "cHRM", "sRGB", "iCCP", "pHYs"].includes(type)) {
      preservedChunks.push(makePngChunk(type, data));
    } else if (type === "IEND") {
      break;
    }

    offset = dataEnd + 4;
  }

  const inflated = zlib.inflateSync(Buffer.concat(idatChunks));
  const stride = width * 4;
  const output = Buffer.alloc(inflated.length);
  const previous = Buffer.alloc(stride);
  const current = Buffer.alloc(stride);
  let inputPosition = 0;
  let outputPosition = 0;

  for (let row = 0; row < height; row++) {
    const filter = inflated[inputPosition++];
    inflated.copy(current, 0, inputPosition, inputPosition + stride);
    inputPosition += stride;

    for (let index = 0; index < stride; index++) {
      const left = index >= 4 ? current[index - 4] : 0;
      const up = previous[index];
      const upLeft = index >= 4 ? previous[index - 4] : 0;
      if (filter === 1) current[index] = (current[index] + left) & 0xff;
      else if (filter === 2) current[index] = (current[index] + up) & 0xff;
      else if (filter === 3) {
        current[index] = (current[index] + Math.floor((left + up) / 2)) & 0xff;
      } else if (filter === 4) {
        current[index] = (current[index] + pngPaeth(left, up, upLeft)) & 0xff;
      } else if (filter !== 0) {
        throw new Error(`unsupported PNG filter ${filter}`);
      }
    }

    output[outputPosition++] = 0;
    for (let index = 0; index < stride; index += 4) {
      const red = current[index];
      const green = current[index + 1];
      const blue = current[index + 2];
      const alpha = current[index + 3];
      const isWhiteMicPixel =
        alpha > 32 && red > 180 && green > 180 && blue > 180;
      output[outputPosition++] = 255;
      output[outputPosition++] = 255;
      output[outputPosition++] = 255;
      output[outputPosition++] = isWhiteMicPixel ? alpha : 0;
    }

    previous.set(current);
  }

  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8;
  ihdr[9] = colorType;

  writeFileSync(
    outputPath,
    Buffer.concat([
      source.subarray(0, 8),
      makePngChunk("IHDR", ihdr),
      ...preservedChunks,
      makePngChunk("IDAT", zlib.deflateSync(output, { level: 9 })),
      makePngChunk("IEND", Buffer.alloc(0)),
    ]),
  );
}

// Icon Composer should own the macOS 26 rounded-square background. The layer
// asset is only the microphone foreground; copying the full app icon here would
// nest a complete icon inside the system icon shape and create a double frame.
writeMicLayerFromMasterIcon(
  path.join(iconsDir, "app-icon.png"),
  path.join(iconComposerDir, "Assets", "OpenKara Mic.png"),
);

await rm(stagingDir, { recursive: true, force: true });
await mkdir(stagingDir, { recursive: true });

execFileSync(
  "xcrun",
  [
    "actool",
    iconComposerDir,
    "--compile",
    stagingDir,
    "--app-icon",
    "OpenKara",
    "--platform",
    "macosx",
    "--minimum-deployment-target",
    "11.0",
    "--target-device",
    "mac",
    "--output-partial-info-plist",
    path.join(stagingDir, "partial.plist"),
    "--output-format",
    "human-readable-text",
  ],
  { stdio: "inherit" },
);

await cp(
  path.join(stagingDir, "Assets.car"),
  path.join(iconsDir, "Assets.car"),
);
await cp(
  path.join(stagingDir, "OpenKara.icns"),
  path.join(iconsDir, "OpenKara.icns"),
);

await rm(stagingDir, { recursive: true, force: true });

console.log(
  "Compiled macOS Liquid Glass assets: src-tauri/icons/Assets.car, OpenKara.icns",
);
