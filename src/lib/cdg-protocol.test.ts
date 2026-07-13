import { describe, expect, test } from "vitest";
import {
  CDG_PROTOCOL_HEADER_SIZE,
  CDG_RGBA_SIZE,
  CDG_HEIGHT,
  CDG_WIDTH,
  parseCdgFrameResponse,
  ensureArrayBuffer,
} from "./cdg-protocol";

/** Build a 32-byte little-endian header for tests. Returns an ArrayBuffer. */
function buildHeader(
  transportGeneration: bigint,
  frameVersion: bigint,
  packetIndex: bigint,
  flags: number,
): ArrayBuffer {
  const header = new Uint8Array(CDG_PROTOCOL_HEADER_SIZE);
  // Magic "OKCG"
  header[0] = 0x4f;
  header[1] = 0x4b;
  header[2] = 0x43;
  header[3] = 0x47;
  // Version 1 (u16 LE)
  header[4] = 1;
  header[5] = 0;
  // Flags (u16 LE)
  header[6] = flags & 0xff;
  header[7] = (flags >> 8) & 0xff;
  // transportGeneration (u64 LE)
  const dv = new DataView(header.buffer);
  dv.setBigUint64(8, transportGeneration, true);
  dv.setBigUint64(16, frameVersion, true);
  dv.setBigUint64(24, packetIndex, true);
  return header.buffer as ArrayBuffer;
}

describe("parseCdgFrameResponse", () => {
  test("returns null for empty response", () => {
    expect(parseCdgFrameResponse(new ArrayBuffer(0))).toBeNull();
  });

  test("parses header-only response (no RGBA flag)", () => {
    const header = buildHeader(42n, 7n, 100n, 0);
    const envelope = parseCdgFrameResponse(header);
    expect(envelope).not.toBeNull();
    expect(envelope!.protocolVersion).toBe(1);
    expect(envelope!.hasRgba).toBe(false);
    expect(envelope!.rgba).toBeNull();
    expect(envelope!.transportGeneration).toBe(42);
    expect(envelope!.frameVersion).toBe(7);
    expect(envelope!.packetIndex).toBe(100);
  });

  test("parses header + RGBA payload response", () => {
    const header = buildHeader(1n, 5n, 200n, 0x01);
    const full = new Uint8Array(CDG_PROTOCOL_HEADER_SIZE + CDG_RGBA_SIZE);
    full.set(new Uint8Array(header), 0);
    // Set a distinctive byte in the RGBA region.
    full[CDG_PROTOCOL_HEADER_SIZE] = 0xab;
    full[CDG_PROTOCOL_HEADER_SIZE + 1] = 0xcd;

    const envelope = parseCdgFrameResponse(full.buffer as ArrayBuffer);
    expect(envelope).not.toBeNull();
    expect(envelope!.hasRgba).toBe(true);
    expect(envelope!.rgba).not.toBeNull();
    expect(envelope!.rgba!.length).toBe(CDG_RGBA_SIZE);
    expect(envelope!.rgba![0]).toBe(0xab);
    expect(envelope!.rgba![1]).toBe(0xcd);
    expect(envelope!.frameVersion).toBe(5);
    expect(envelope!.packetIndex).toBe(200);
  });

  test("throws on invalid magic bytes", () => {
    const header = new Uint8Array(CDG_PROTOCOL_HEADER_SIZE);
    header[0] = 0x00; // wrong magic
    header[1] = 0x00;
    header[2] = 0x00;
    header[3] = 0x00;
    expect(() => parseCdgFrameResponse(header.buffer as ArrayBuffer)).toThrow(
      "invalid magic bytes",
    );
  });

  test("throws on response shorter than header", () => {
    const short = new Uint8Array(10);
    expect(() => parseCdgFrameResponse(short.buffer as ArrayBuffer)).toThrow(
      "too short",
    );
  });

  test("throws when RGBA flag set but payload too short", () => {
    const header = buildHeader(1n, 1n, 1n, 0x01);
    const short = new Uint8Array(CDG_PROTOCOL_HEADER_SIZE + 10);
    short.set(new Uint8Array(header), 0);
    expect(() => parseCdgFrameResponse(short.buffer as ArrayBuffer)).toThrow(
      "RGBA payload too short",
    );
  });
});

describe("ensureArrayBuffer", () => {
  test("passes through ArrayBuffer", () => {
    const buf = new ArrayBuffer(4);
    expect(ensureArrayBuffer(buf)).toBe(buf);
  });

  test("converts Uint8Array to ArrayBuffer", () => {
    const arr = new Uint8Array([1, 2, 3]);
    const buf = ensureArrayBuffer(arr);
    expect(buf.byteLength).toBe(3);
    expect(new Uint8Array(buf)).toEqual(new Uint8Array([1, 2, 3]));
  });

  test("converts number[] to ArrayBuffer", () => {
    const arr = [10, 20, 30];
    const buf = ensureArrayBuffer(arr);
    expect(buf.byteLength).toBe(3);
    expect(new Uint8Array(buf)).toEqual(new Uint8Array([10, 20, 30]));
  });

  test("returns empty ArrayBuffer for unknown types", () => {
    expect(ensureArrayBuffer("hello").byteLength).toBe(0);
    expect(ensureArrayBuffer(null).byteLength).toBe(0);
    expect(ensureArrayBuffer(undefined).byteLength).toBe(0);
  });
});

describe("CDG dimensions", () => {
  test("RGBA size matches dimensions", () => {
    expect(CDG_RGBA_SIZE).toBe(CDG_WIDTH * CDG_HEIGHT * 4);
    expect(CDG_WIDTH).toBe(288);
    expect(CDG_HEIGHT).toBe(192);
  });
});
