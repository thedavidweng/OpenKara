export const CDG_WIDTH = 288;
export const CDG_HEIGHT = 192;

let cdgCanvasEl: HTMLCanvasElement | null = null;
let cdgCanvasCtx: CanvasRenderingContext2D | null = null;
let lastFrameBytes: Uint8ClampedArray | null = null;

let reusableImageData: ImageData | null = null;

function ensureImageData(): ImageData {
  if (!reusableImageData) {
    reusableImageData = new ImageData(CDG_WIDTH, CDG_HEIGHT);
  }

  return reusableImageData;
}

function paintBytes(bytes: Uint8ClampedArray | Uint8Array): void {
  if (!cdgCanvasCtx || !cdgCanvasEl) return;

  const imageData = ensureImageData();
  imageData.data.set(bytes);
  cdgCanvasCtx.putImageData(imageData, 0, 0);
}

export function setCdgCanvas(canvas: HTMLCanvasElement | null): void {
  cdgCanvasEl = canvas;
  cdgCanvasCtx = canvas?.getContext("2d") ?? null;
  reusableImageData = null;

  if (lastFrameBytes) {
    paintBytes(lastFrameBytes);
  }
}

export function drawFrame(frame: ArrayBuffer | Uint8Array): void {
  if (frame instanceof Uint8Array) {
    lastFrameBytes = new Uint8ClampedArray(
      frame.buffer,
      frame.byteOffset,
      frame.byteLength,
    );
  } else {
    lastFrameBytes = new Uint8ClampedArray(frame);
  }
  paintBytes(lastFrameBytes);
}

export function clearFrame(): void {
  lastFrameBytes = null;
  cdgCanvasCtx?.clearRect(0, 0, CDG_WIDTH, CDG_HEIGHT);
}
