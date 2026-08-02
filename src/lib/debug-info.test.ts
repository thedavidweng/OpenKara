import { describe, expect, test, vi } from "vitest";
import type { DebugInfo } from "@/types/ipc";
import { copyDebugInfo, formatDebugInfo } from "./debug-info";

vi.mock("@/lib/tauri", () => ({ getDebugInfo: vi.fn() }));

const sample: DebugInfo = {
  app_version: "0.9.1",
  build_sha: "abc1234",
  os: "macos",
  arch: "aarch64",
  catalog_generation: 9,
  catalog_release_id: "2026-07-25-006",
  model_variant: "htdemucs",
  model_state: "ready",
  model_installed: true,
  model_installed_version: "model-v2.1.0",
  model_pinned_version: "model-v2.1.0",
  model_path: "/tmp/models/htdemucs.onnx",
  runtime_state: "ready",
  runtime_version: "v1.27.1",
  runtime_artifact_id: "onnxruntime-1.27.1-openkara-aarch64-apple-darwin",
  runtime_target_triple: "aarch64-apple-darwin",
  execution_provider: "xnnpack",
  log_file: "/Users/me/Library/Logs/com.openkara.desktop/openkara.<date>.log",
};

describe("formatDebugInfo", () => {
  test("includes every field value", () => {
    const text = formatDebugInfo(sample);
    for (const fragment of [
      "OpenKara · About",
      "0.9.1",
      "abc1234",
      "macos",
      "aarch64",
      "Catalog: 9",
      "2026-07-25-006",
      "htdemucs",
      "ready",
      "model-v2.1.0",
      "v1.27.1",
      "onnxruntime-1.27.1-openkara-aarch64-apple-darwin",
      "aarch64-apple-darwin",
      "xnnpack",
      "openkara.<date>.log",
    ]) {
      expect(text).toContain(fragment);
    }
  });

  test("reports an uninstalled model without a version", () => {
    const text = formatDebugInfo({
      ...sample,
      model_installed: false,
      model_installed_version: null,
    });
    expect(text).toContain("Model: htdemucs · ready · — · model-v2.1.0");
  });

  test("renders an empty marker when no runtime artifact is active", () => {
    const text = formatDebugInfo({ ...sample, runtime_artifact_id: null });
    expect(text).toContain("Runtime: ready · v1.27.1 · — ·");
  });

  test("is stable, newline-delimited plain text", () => {
    const lines = formatDebugInfo(sample).split("\n");
    expect(lines[0]).toBe("OpenKara · About");
    expect(lines).toHaveLength(8);
  });
});

describe("copyDebugInfo", () => {
  test("fetches the snapshot and writes its formatted text", async () => {
    const fetchDebugInfo = vi.fn(async () => sample);
    const writeText = vi.fn(async () => {});

    await copyDebugInfo({ fetchDebugInfo, writeText });

    expect(fetchDebugInfo).toHaveBeenCalledOnce();
    expect(writeText).toHaveBeenCalledWith(formatDebugInfo(sample));
  });

  test("propagates a fetch failure to the caller", async () => {
    const fetchDebugInfo = vi.fn(async () => {
      throw new Error("ipc down");
    });
    const writeText = vi.fn(async () => {});

    await expect(copyDebugInfo({ fetchDebugInfo, writeText })).rejects.toThrow(
      "ipc down",
    );
    expect(writeText).not.toHaveBeenCalled();
  });
});
