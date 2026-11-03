// @vitest-environment jsdom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { beforeEach, describe, expect, test } from "vite-plus/test";
import { useRuntimeBootstrapStore } from "@/stores/runtime-bootstrap-store";
import { createRecordingRuntimeEventSource } from "@/runtime/event-source";
import type { RuntimeBootstrapStatusSnapshot } from "@/types/ipc";
import { useBootstrapEvents } from "./bootstrap-events";

function makeSnapshot(
  overrides: Partial<RuntimeBootstrapStatusSnapshot> = {},
): RuntimeBootstrapStatusSnapshot {
  return {
    state: "ready",
    runtime_path: "/runtimes/test",
    downloaded_bytes: null,
    total_bytes: null,
    version: "1.0.0",
    active_artifact_id: "rt-cpu",
    target_triple: "x86_64-pc-windows-msvc",
    candidate_version: null,
    restart_required: false,
    error: null,
    ...overrides,
  };
}

async function renderHook(fn: () => void) {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);
  function Component() {
    fn();
    return null;
  }
  await act(async () => {
    root.render(<Component />);
    await Promise.resolve();
  });
  return { root, container };
}

describe("useBootstrapEvents", () => {
  let source: ReturnType<typeof createRecordingRuntimeEventSource>;

  beforeEach(() => {
    useRuntimeBootstrapStore.setState({ status: null });
    source = createRecordingRuntimeEventSource();
  });

  test("updates the runtime bootstrap store", async () => {
    await renderHook(() => useBootstrapEvents(true, source));
    const snapshot = makeSnapshot({
      state: "downloading",
      downloaded_bytes: 10,
    });

    await act(async () => {
      source.emit("runtime-bootstrap-progress", snapshot);
      await Promise.resolve();
    });

    expect(useRuntimeBootstrapStore.getState().status?.state).toBe(
      "downloading",
    );
  });
});
