/**
 * Binary CDG frame protocol parser.
 *
 * The backend returns a 32-byte little-endian header followed by an optional
 * RGBA payload (288×192×4 = 221,184 bytes). The header carries:
 *
 *   bytes 0..4   magic ("OKCG")
 *   bytes 4..6   protocol version (u16 LE)
 *   bytes 6..8   flags (u16 LE; bit 0 = RGBA payload present)
 *   bytes 8..16  transport generation (u64 LE)
 *   bytes 16..24 frame version (u64 LE)
 *   bytes 24..32 packet index (u64 LE)
 *
 * A response of 0 bytes means no active CDG, stale song/generation, or error.
 * A response of exactly 32 bytes (no RGBA flag) means the caller already has
 * the current frame.
 *
 * RATIONALE: The previous protocol returned a raw RGBA ArrayBuffer with no
 * metadata. The frontend had no way to distinguish "no CDG active" from
 * "frame unchanged" from "stale song". The binary envelope solves this by
 * carrying transport generation and frame version in a fixed-size header,
 * enabling the frontend to skip redundant redraws and discard stale frames.
 */

/** Magic bytes identifying the CDG binary protocol. */
const PROTOCOL_MAGIC = [0x4f, 0x4b, 0x43, 0x47]; // "OKCG"
/** Header size in bytes. */
export const CDG_PROTOCOL_HEADER_SIZE = 32;
/** Flag bit 0: RGBA payload present. */
const FLAG_RGBA_PRESENT = 0x01;

/** CDG visible frame dimensions. */
export const CDG_WIDTH = 288;
export const CDG_HEIGHT = 192;
/** RGBA frame size in bytes (288 * 192 * 4). */
export const CDG_RGBA_SIZE = CDG_WIDTH * CDG_HEIGHT * 4;

/** Parsed CDG frame envelope. */
export interface CdgFrameEnvelope {
  /** Protocol version from the header. */
  protocolVersion: number;
  /** Whether an RGBA payload is included. */
  hasRgba: boolean;
  /** Transport generation from the backend. */
  transportGeneration: number;
  /** Monotonically increasing frame version for this timeline. */
  frameVersion: number;
  /** Exclusive packet cursor (packets processed so far). */
  packetIndex: number;
  /** RGBA frame data (221,184 bytes), or null if no payload. */
  rgba: Uint8Array | null;
}

/**
 * Parse a binary CDG frame response into a structured envelope.
 *
 * Returns `null` for empty responses (no active CDG / stale / error).
 * Throws if the response is too short or has an invalid magic bytes.
 */
export function parseCdgFrameResponse(
  data: ArrayBuffer,
): CdgFrameEnvelope | null {
  if (data.byteLength === 0) {
    return null;
  }

  if (data.byteLength < CDG_PROTOCOL_HEADER_SIZE) {
    throw new Error(
      `CDG protocol: response too short (${data.byteLength} bytes, expected >= ${CDG_PROTOCOL_HEADER_SIZE})`,
    );
  }

  const view = new DataView(data);

  // Verify magic bytes.
  for (let i = 0; i < 4; i++) {
    if (view.getUint8(i) !== PROTOCOL_MAGIC[i]) {
      throw new Error("CDG protocol: invalid magic bytes");
    }
  }

  const protocolVersion = view.getUint16(4, true);
  const flags = view.getUint16(6, true);
  const transportGeneration = Number(view.getBigUint64(8, true));
  const frameVersion = Number(view.getBigUint64(16, true));
  const packetIndex = Number(view.getBigUint64(24, true));
  const hasRgba = (flags & FLAG_RGBA_PRESENT) !== 0;

  let rgba: Uint8Array | null = null;
  if (hasRgba) {
    const expectedSize = CDG_PROTOCOL_HEADER_SIZE + CDG_RGBA_SIZE;
    if (data.byteLength < expectedSize) {
      throw new Error(
        `CDG protocol: RGBA payload too short (${data.byteLength} bytes, expected ${expectedSize})`,
      );
    }
    rgba = new Uint8Array(data, CDG_PROTOCOL_HEADER_SIZE, CDG_RGBA_SIZE);
  }

  return {
    protocolVersion,
    hasRgba,
    transportGeneration,
    frameVersion,
    packetIndex,
    rgba,
  };
}

/**
 * Normalize the IPC response to an ArrayBuffer.
 *
 * PERF: The backend returns raw bytes via `tauri::ipc::Response`, which
 * **should** arrive as an `ArrayBuffer` on desktop platforms. However, Tauri's
 * IPC bridge may occasionally deliver it as a `number[]` (JSON-serialized
 * Vec<u8>) depending on the protocol path. This function handles both cases
 * so CDG rendering is robust regardless of IPC serialization behavior.
 */
export function ensureArrayBuffer(result: unknown): ArrayBuffer {
  if (result instanceof ArrayBuffer) return result;
  if (ArrayBuffer.isView(result)) {
    const view = result as ArrayBufferView;
    const copy = new Uint8Array(view.byteLength);
    copy.set(new Uint8Array(view.buffer, view.byteOffset, view.byteLength));
    return copy.buffer;
  }
  if (Array.isArray(result)) {
    return new Uint8Array(result as number[]).buffer;
  }
  return new ArrayBuffer(0);
}
