const PROTOCOL_MAGIC = [0x4f, 0x4b, 0x43, 0x47]; // "OKCG"
export const CDG_PROTOCOL_HEADER_SIZE = 32;
const FLAG_RGBA_PRESENT = 0x01;

export const CDG_WIDTH = 288;
export const CDG_HEIGHT = 192;
export const CDG_RGBA_SIZE = CDG_WIDTH * CDG_HEIGHT * 4;

export interface CdgFrameEnvelope {
  protocolVersion: number;
  hasRgba: boolean;
  transportGeneration: number;
  frameVersion: number;
  packetIndex: number;
  rgba: Uint8Array | null;
}

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

/** Tauri IPC may deliver bytes as ArrayBuffer or number[]. */
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
