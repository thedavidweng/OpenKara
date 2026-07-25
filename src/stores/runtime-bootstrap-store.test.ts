import { beforeEach, describe, expect, test, vi } from "vitest";
import type { RuntimeBootstrapStatusSnapshot } from "@/types/ipc";

vi.mock("@/lib/tauri", () => ({
  getRuntimeBootstrapStatus: vi.fn(),
}));

import { useRuntimeBootstrapStore } from "./runtime-bootstrap-store";
import * as api from "@/lib/tauri";

const mockGetRuntimeBootstrapStatus = vi.mocked(api.getRuntimeBootstrapStatus);

function makeSnapshot(
  overrides: Partial<RuntimeBootstrapStatusSnapshot> = {},
): RuntimeBootstrapStatusSnapshot {
  return {
    state: "downloading",
    runtime_path: "/runtimes/node-v20",
    downloaded_bytes: 0,
    total_bytes: null,
    version: "1.0.0",
    active_artifact_id: null,
    target_triple: "aarch64-apple-darwin",
    candidate_version: null,
    restart_required: false,
    error: null,
    ...overrides,
  };
}

describe("runtime-bootstrap-store", () => {
  beforeEach(() => {
    useRuntimeBootstrapStore.setState({ status: null });
    vi.clearAllMocks();
  });

  describe("initial state", () => {
    test("status is null", () => {
      expect(useRuntimeBootstrapStore.getState().status).toBeNull();
    });
  });

  describe("updateStatus", () => {
    test("sets status when previous is null", () => {
      const incoming = makeSnapshot({
        downloaded_bytes: 500,
        total_bytes: 1000,
      });

      useRuntimeBootstrapStore.getState().updateStatus(incoming);

      expect(useRuntimeBootstrapStore.getState().status).toEqual(incoming);
    });

    test("merges downloading states with same runtime_path: takes max downloaded_bytes", () => {
      useRuntimeBootstrapStore.setState({
        status: makeSnapshot({
          downloaded_bytes: 800,
          total_bytes: 1000,
        }),
      });

      // Incoming reports lower bytes (out-of-order event)
      useRuntimeBootstrapStore.getState().updateStatus(
        makeSnapshot({
          downloaded_bytes: 500,
          total_bytes: 1000,
        }),
      );

      const status = useRuntimeBootstrapStore.getState().status!;
      expect(status.downloaded_bytes).toBe(800);
      expect(status.total_bytes).toBe(1000);
    });

    test("merges downloading states: accepts higher downloaded_bytes", () => {
      useRuntimeBootstrapStore.setState({
        status: makeSnapshot({
          downloaded_bytes: 500,
          total_bytes: 1000,
        }),
      });

      useRuntimeBootstrapStore.getState().updateStatus(
        makeSnapshot({
          downloaded_bytes: 900,
          total_bytes: 1000,
        }),
      );

      expect(useRuntimeBootstrapStore.getState().status!.downloaded_bytes).toBe(
        900,
      );
    });

    test("merges downloading states: uses incoming total_bytes when available", () => {
      useRuntimeBootstrapStore.setState({
        status: makeSnapshot({
          downloaded_bytes: 500,
          total_bytes: 1000,
        }),
      });

      useRuntimeBootstrapStore.getState().updateStatus(
        makeSnapshot({
          downloaded_bytes: 600,
          total_bytes: 2000,
        }),
      );

      expect(useRuntimeBootstrapStore.getState().status!.total_bytes).toBe(
        2000,
      );
    });

    test("merges downloading states: falls back to previous total_bytes when incoming has none", () => {
      useRuntimeBootstrapStore.setState({
        status: makeSnapshot({
          downloaded_bytes: 500,
          total_bytes: 1000,
        }),
      });

      useRuntimeBootstrapStore.getState().updateStatus(
        makeSnapshot({
          downloaded_bytes: 600,
          total_bytes: null,
        }),
      );

      expect(useRuntimeBootstrapStore.getState().status!.total_bytes).toBe(
        1000,
      );
    });

    test("merges downloading states: handles null downloaded_bytes on previous", () => {
      useRuntimeBootstrapStore.setState({
        status: makeSnapshot({
          downloaded_bytes: null,
          total_bytes: 1000,
        }),
      });

      useRuntimeBootstrapStore.getState().updateStatus(
        makeSnapshot({
          downloaded_bytes: 400,
          total_bytes: 1000,
        }),
      );

      expect(useRuntimeBootstrapStore.getState().status!.downloaded_bytes).toBe(
        400,
      );
    });

    test("replaces status when state transitions (downloading -> ready)", () => {
      useRuntimeBootstrapStore.setState({
        status: makeSnapshot({
          downloaded_bytes: 500,
          total_bytes: 1000,
        }),
      });

      const readyStatus = makeSnapshot({
        state: "ready",
        downloaded_bytes: null,
        total_bytes: null,
      });

      useRuntimeBootstrapStore.getState().updateStatus(readyStatus);

      expect(useRuntimeBootstrapStore.getState().status).toEqual(readyStatus);
    });

    test("carries candidate lifecycle fields through a state transition", () => {
      useRuntimeBootstrapStore.setState({
        status: makeSnapshot({
          downloaded_bytes: 500,
          total_bytes: 1000,
        }),
      });

      const candidateReady = makeSnapshot({
        state: "candidate_ready_restart_required",
        downloaded_bytes: null,
        total_bytes: null,
        active_artifact_id: "rt-1.27.1",
        candidate_version: "v1.28.0",
        restart_required: true,
      });

      useRuntimeBootstrapStore.getState().updateStatus(candidateReady);

      const status = useRuntimeBootstrapStore.getState().status!;
      expect(status.state).toBe("candidate_ready_restart_required");
      expect(status.candidate_version).toBe("v1.28.0");
      expect(status.restart_required).toBe(true);
      expect(status.active_artifact_id).toBe("rt-1.27.1");
    });

    test("replaces status when runtime_path changes", () => {
      useRuntimeBootstrapStore.setState({
        status: makeSnapshot({
          runtime_path: "/runtimes/node-v18",
          downloaded_bytes: 800,
          total_bytes: 1000,
        }),
      });

      const newStatus = makeSnapshot({
        runtime_path: "/runtimes/node-v20",
        downloaded_bytes: 100,
        total_bytes: 2000,
      });

      useRuntimeBootstrapStore.getState().updateStatus(newStatus);

      expect(useRuntimeBootstrapStore.getState().status).toEqual(newStatus);
    });
  });

  describe("loadStatus", () => {
    test("calls getRuntimeBootstrapStatus and sets status", async () => {
      const snapshot = makeSnapshot({
        state: "ready",
        downloaded_bytes: null,
        total_bytes: null,
      });
      mockGetRuntimeBootstrapStatus.mockResolvedValue(snapshot);

      await useRuntimeBootstrapStore.getState().loadStatus();

      expect(mockGetRuntimeBootstrapStatus).toHaveBeenCalledOnce();
      expect(useRuntimeBootstrapStore.getState().status).toEqual(snapshot);
    });

    test("sets null status when API returns null", async () => {
      mockGetRuntimeBootstrapStatus.mockResolvedValue(null as never);

      await useRuntimeBootstrapStore.getState().loadStatus();

      expect(useRuntimeBootstrapStore.getState().status).toBeNull();
    });
  });
});
