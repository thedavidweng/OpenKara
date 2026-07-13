import { describe, expect, test, vi } from "vitest";
import {
  createCdgSyncChannel,
  postCdgClear,
  postCdgFrame,
  postCdgStatus,
  startCdgSyncReceiver,
  startCdgSyncRequestListener,
  type CdgSyncChannel,
  type CdgSyncMessage,
} from "./cdg-sync-channel";

function createMockChannel(): CdgSyncChannel {
  return {
    postMessage: vi.fn(),
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    close: vi.fn(),
  };
}

function makeEvent(data: CdgSyncMessage): MessageEvent<CdgSyncMessage> {
  return { data } as MessageEvent<CdgSyncMessage>;
}

function extractListener(channel: CdgSyncChannel) {
  return (channel.addEventListener as ReturnType<typeof vi.fn>).mock
    .calls[0][1] as (event: MessageEvent<CdgSyncMessage>) => void;
}

describe("createCdgSyncChannel", () => {
  test("calls factory with channel name and returns result", () => {
    const mockChannel = createMockChannel();
    const factory = vi.fn().mockReturnValue(mockChannel);

    const result = createCdgSyncChannel(factory);

    expect(factory).toHaveBeenCalledWith("openkara-cdg-sync-v2");
    expect(result).toBe(mockChannel);
  });

  test("returns null when factory throws", () => {
    const factory = vi.fn().mockImplementation(() => {
      throw new Error("boom");
    });

    const result = createCdgSyncChannel(factory);

    expect(result).toBeNull();
  });

  test("returns null when BroadcastChannel is undefined", () => {
    const original = globalThis.BroadcastChannel;
    Reflect.deleteProperty(globalThis, "BroadcastChannel");

    const factory = vi.fn();
    const result = createCdgSyncChannel(factory);

    expect(result).toBeNull();
    expect(factory).not.toHaveBeenCalled();

    globalThis.BroadcastChannel = original;
  });
});

describe("postCdgStatus", () => {
  test("posts {type:'status', payload} to channel", () => {
    const channel = createMockChannel();

    postCdgStatus(channel, { songId: "abc", hasCdg: true });

    expect(channel.postMessage).toHaveBeenCalledWith({
      type: "status",
      payload: { songId: "abc", hasCdg: true },
    });
  });

  test("no-op with null channel (no throw)", () => {
    expect(() =>
      postCdgStatus(null, { songId: null, hasCdg: false }),
    ).not.toThrow();
  });
});

describe("postCdgFrame", () => {
  test("posts {type:'frame', payload} to channel", () => {
    const channel = createMockChannel();
    const payload = {
      rgba: new Uint8Array(8),
      frameVersion: 3,
      transportGeneration: 1,
    };

    postCdgFrame(channel, payload);

    expect(channel.postMessage).toHaveBeenCalledWith({
      type: "frame",
      payload,
    });
  });
});

describe("postCdgClear", () => {
  test("posts {type:'clear'} to channel", () => {
    const channel = createMockChannel();

    postCdgClear(channel);

    expect(channel.postMessage).toHaveBeenCalledWith({ type: "clear" });
  });
});

describe("startCdgSyncRequestListener", () => {
  test("on 'request-sync' message, calls getSnapshot and posts status + frame", () => {
    const channel = createMockChannel();
    const frame = {
      rgba: new Uint8Array(16),
      frameVersion: 1,
      transportGeneration: 1,
    };
    const status = { songId: "s1", hasCdg: true };
    const getSnapshot = vi.fn().mockReturnValue({ status, frame });

    startCdgSyncRequestListener({ channel, getSnapshot });
    const listener = extractListener(channel);

    listener(makeEvent({ type: "request-sync" }));

    expect(getSnapshot).toHaveBeenCalled();
    expect(channel.postMessage).toHaveBeenCalledWith({
      type: "status",
      payload: status,
    });
    expect(channel.postMessage).toHaveBeenCalledWith({
      type: "frame",
      payload: frame,
    });
  });

  test("ignores non-'request-sync' messages", () => {
    const channel = createMockChannel();
    const getSnapshot = vi.fn();

    startCdgSyncRequestListener({ channel, getSnapshot });
    const listener = extractListener(channel);

    listener(makeEvent({ type: "clear" }));
    listener(
      makeEvent({ type: "status", payload: { songId: null, hasCdg: false } }),
    );
    listener(
      makeEvent({
        type: "frame",
        payload: {
          rgba: new Uint8Array(0),
          frameVersion: 0,
          transportGeneration: 0,
        },
      }),
    );

    expect(getSnapshot).not.toHaveBeenCalled();
    expect(channel.postMessage).not.toHaveBeenCalled();
  });
});

describe("startCdgSyncReceiver", () => {
  test("on 'frame' calls onFrame, on 'clear' calls onClear, on 'status' calls onStatus", () => {
    const channel = createMockChannel();
    const onFrame = vi.fn();
    const onClear = vi.fn();
    const onStatus = vi.fn();

    startCdgSyncReceiver({ channel, onFrame, onClear, onStatus });
    const listener = extractListener(channel);

    const framePayload = {
      rgba: new Uint8Array(4),
      frameVersion: 2,
      transportGeneration: 1,
    };
    listener(makeEvent({ type: "frame", payload: framePayload }));
    expect(onFrame).toHaveBeenCalledWith(framePayload);

    listener(makeEvent({ type: "clear" }));
    expect(onClear).toHaveBeenCalled();

    const statusPayload = { songId: "x", hasCdg: false };
    listener(makeEvent({ type: "status", payload: statusPayload }));
    expect(onStatus).toHaveBeenCalledWith(statusPayload);
  });

  test("posts initial 'request-sync' on setup", () => {
    const channel = createMockChannel();

    startCdgSyncReceiver({
      channel,
      onFrame: vi.fn(),
      onClear: vi.fn(),
      onStatus: vi.fn(),
    });

    expect(channel.postMessage).toHaveBeenCalledWith({
      type: "request-sync",
    });
  });
});

describe("returned cleanup function", () => {
  test("removes listener", () => {
    const channel = createMockChannel();

    const cleanup = startCdgSyncReceiver({
      channel,
      onFrame: vi.fn(),
      onClear: vi.fn(),
      onStatus: vi.fn(),
    });

    const addedListener = extractListener(channel);
    cleanup();

    expect(channel.removeEventListener).toHaveBeenCalledWith(
      "message",
      addedListener,
    );
  });
});
