// @vitest-environment jsdom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test } from "vitest";
import { useNotificationStore } from "@/stores/notification-store";
import { useRuntimeBootstrapStore } from "@/stores/runtime-bootstrap-store";
import { createRecordingRuntimeEventSource } from "@/runtime/event-source";
import type { RuntimeBootstrapStatusSnapshot } from "@/types/ipc";
import { useBootstrapEvents } from "./bootstrap-events";

const NOTICE_STORAGE_KEY = "openkara.cpuFallbackNoticeShown";

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

function withCpuFallbackNotice(
  snapshot: RuntimeBootstrapStatusSnapshot,
): RuntimeBootstrapStatusSnapshot {
  return {
    ...snapshot,
    cpu_fallback_notice: "cpu-runtime-fallback-after-directml-timeout",
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

describe("useBootstrapEvents cpu fallback notice", () => {
  let source: ReturnType<typeof createRecordingRuntimeEventSource>;

  beforeEach(() => {
    localStorage.removeItem(NOTICE_STORAGE_KEY);
    useNotificationStore.setState({ notifications: [] });
    useRuntimeBootstrapStore.setState({ status: null });
    source = createRecordingRuntimeEventSource();
  });

  afterEach(() => {
    localStorage.removeItem(NOTICE_STORAGE_KEY);
  });

  test("fires a notification once when cpu_fallback_notice is present", async () => {
    await renderHook(() => useBootstrapEvents(true, source));

    await act(async () => {
      source.emit(
        "runtime-bootstrap-ready",
        withCpuFallbackNotice(makeSnapshot()),
      );
      await Promise.resolve();
    });

    expect(useNotificationStore.getState().notifications).toHaveLength(1);
    expect(useNotificationStore.getState().notifications[0].type).toBe("info");
    expect(localStorage.getItem(NOTICE_STORAGE_KEY)).toBe("1");

    await act(async () => {
      source.emit(
        "runtime-bootstrap-progress",
        withCpuFallbackNotice(makeSnapshot()),
      );
      await Promise.resolve();
    });

    expect(useNotificationStore.getState().notifications).toHaveLength(1);
  });

  test("does not fire when cpu_fallback_notice is absent", async () => {
    await renderHook(() => useBootstrapEvents(true, source));

    await act(async () => {
      source.emit("runtime-bootstrap-ready", makeSnapshot());
      await Promise.resolve();
    });

    expect(useNotificationStore.getState().notifications).toHaveLength(0);
    expect(localStorage.getItem(NOTICE_STORAGE_KEY)).toBeNull();
  });

  test("does not fire again when already shown", async () => {
    localStorage.setItem(NOTICE_STORAGE_KEY, "1");
    await renderHook(() => useBootstrapEvents(true, source));

    await act(async () => {
      source.emit(
        "runtime-bootstrap-ready",
        withCpuFallbackNotice(makeSnapshot()),
      );
      await Promise.resolve();
    });

    expect(useNotificationStore.getState().notifications).toHaveLength(0);
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
