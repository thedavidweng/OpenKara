/** CDG visible frame dimensions. */
export const CDG_WIDTH = 288;
export const CDG_HEIGHT = 192;

/**
 * Module-level canvas element reference. The CdgCanvas component registers its
 * canvas here so that the rAF loop can paint directly without going through
 * React/Zustand state updates. CDG can update many times per second, so pushing
 * every frame through React would add avoidable render churn.
 *
 * Each Tauri WebviewWindow runs its own JS context, so the module-level
 * variables are independent between the main window and the fullscreen window.
 */
let cdgCanvasEl: HTMLCanvasElement | null = null;
let cdgCanvasCtx: CanvasRenderingContext2D | null = null;
let lastFrameBytes: Uint8ClampedArray | null = null;

/**
 * PERF: Pre-allocated ImageData reused across frames. Creating a new
 * `ImageData` (221 KB) on every frame at 30fps produces ~6.5 MB/s of GC
 * pressure. Reusing one instance eliminates this entirely. Do not change
 * `drawFrame` to allocate a new `ImageData` per call.
 */
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
  // Reset pre-allocated ImageData when canvas changes (new context).
  reusableImageData = null;

  if (lastFrameBytes) {
    paintBytes(lastFrameBytes);
  }
}

/**
 * Paint a raw RGBA frame onto the CDG canvas.
 *
 * Accepts either an `ArrayBuffer` (legacy path) or a `Uint8Array` view into
 * the RGBA payload of the binary protocol envelope (preferred path). When a
 * `Uint8Array` is passed, it is a zero-copy view into the IPC response buffer.
 *
 * PERF: This is the **performance-critical rendering path** for the main
 * window. The backend returns raw bytes via `tauri::ipc::Response` and the
 * IPC bridge delivers them as an `ArrayBuffer`. We wrap the buffer in a
 * `Uint8Array` view (O(1), no copy) and `.set()` it into the pre-allocated
 * `ImageData`. This avoids:
 *   1. Base64 decoding (`atob` + O(n) `charCodeAt` loop)
 *   2. Per-frame `ImageData` allocation (221 KB GC pressure)
 *
 * Do not revert to base64 string input or per-frame `new ImageData()` —
 * both were the primary CDG performance bottlenecks before this optimization.
 */
export function drawFrame(frame: ArrayBuffer | Uint8Array): void {
  if (frame instanceof Uint8Array) {
    // Zero-copy view into the binary protocol's RGBA payload.
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
