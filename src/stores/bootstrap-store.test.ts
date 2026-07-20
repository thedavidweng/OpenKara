import { beforeEach, describe, expect, test, vi } from "vitest";

vi.mock("@/lib/tauri", () => ({
  getModelBootstrapStatus: vi.fn(),
}));

import { useBootstrapStore } from "./bootstrap-store";

describe("bootstrap-store updateStatus", () => {
  beforeEach(() => {
    useBootstrapStore.setState({ status: null });
  });

  test("sets status when previous is null", () => {
    const incoming = {
      state: "downloading" as const,
      model_path: "/models/htdemucs.onnx",
      downloaded_bytes: 500,
      total_bytes: 1000,
      error: null,
    };

    useBootstrapStore.getState().updateStatus(incoming);

    expect(useBootstrapStore.getState().status).toEqual(incoming);
  });

  test("preserves max downloaded_bytes for same model in downloading state", () => {
    useBootstrapStore.setState({
      status: {
        state: "downloading",
        model_path: "/models/htdemucs.onnx",
        downloaded_bytes: 800,
        total_bytes: 1000,
        error: null,
      },
    });

    // Incoming reports lower bytes (out-of-order event)
    useBootstrapStore.getState().updateStatus({
      state: "downloading",
      model_path: "/models/htdemucs.onnx",
      downloaded_bytes: 500,
      total_bytes: 1000,
      error: null,
    });

    const status = useBootstrapStore.getState().status!;
    expect(status.downloaded_bytes).toBe(800);
    expect(status.total_bytes).toBe(1000);
  });

  test("accepts higher downloaded_bytes for same model", () => {
    useBootstrapStore.setState({
      status: {
        state: "downloading",
        model_path: "/models/htdemucs.onnx",
        downloaded_bytes: 500,
        total_bytes: 1000,
        error: null,
      },
    });

    useBootstrapStore.getState().updateStatus({
      state: "downloading",
      model_path: "/models/htdemucs.onnx",
      downloaded_bytes: 900,
      total_bytes: 1000,
      error: null,
    });

    const status = useBootstrapStore.getState().status!;
    expect(status.downloaded_bytes).toBe(900);
  });

  test("replaces status when model_path changes", () => {
    useBootstrapStore.setState({
      status: {
        state: "downloading",
        model_path: "/models/htdemucs.onnx",
        downloaded_bytes: 800,
        total_bytes: 1000,
        error: null,
      },
    });

    const newStatus = {
      state: "downloading" as const,
      model_path: "/models/htdemucs_ft.onnx",
      downloaded_bytes: 100,
      total_bytes: 2000,
      error: null,
    };

    useBootstrapStore.getState().updateStatus(newStatus);

    expect(useBootstrapStore.getState().status).toEqual(newStatus);
  });

  test("replaces status when incoming state is not downloading", () => {
    useBootstrapStore.setState({
      status: {
        state: "downloading",
        model_path: "/models/htdemucs.onnx",
        downloaded_bytes: 500,
        total_bytes: 1000,
        error: null,
      },
    });

    const completed = {
      state: "ready" as const,
      model_path: "/models/htdemucs.onnx",
      downloaded_bytes: null,
      total_bytes: null,
      error: null,
    };

    useBootstrapStore.getState().updateStatus(completed);

    expect(useBootstrapStore.getState().status).toEqual(completed);
  });

  test("uses incoming total_bytes when available", () => {
    useBootstrapStore.setState({
      status: {
        state: "downloading",
        model_path: "/models/htdemucs.onnx",
        downloaded_bytes: 500,
        total_bytes: 1000,
        error: null,
      },
    });

    useBootstrapStore.getState().updateStatus({
      state: "downloading",
      model_path: "/models/htdemucs.onnx",
      downloaded_bytes: 600,
      total_bytes: 2000,
      error: null,
    });

    expect(useBootstrapStore.getState().status!.total_bytes).toBe(2000);
  });

  test("falls back to previous total_bytes when incoming has none", () => {
    useBootstrapStore.setState({
      status: {
        state: "downloading",
        model_path: "/models/htdemucs.onnx",
        downloaded_bytes: 500,
        total_bytes: 1000,
        error: null,
      },
    });

    useBootstrapStore.getState().updateStatus({
      state: "downloading",
      model_path: "/models/htdemucs.onnx",
      downloaded_bytes: 600,
      total_bytes: null,
      error: null,
    });

    expect(useBootstrapStore.getState().status!.total_bytes).toBe(1000);
  });
});
