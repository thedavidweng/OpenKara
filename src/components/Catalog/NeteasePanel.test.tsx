// @vitest-environment jsdom
import userEvent from "@testing-library/user-event";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { NeteasePanel } from "./NeteasePanel";
import type { StreamingSessionSnapshot } from "@/types/ipc";

const { mockState } = vi.hoisted(() => ({
  mockState: {
    session: null as StreamingSessionSnapshot | null,
    liked: [] as Array<{
      source_id: "netease";
      remote_track_id: string;
      title: string;
      artist: string;
      album: string | null;
      duration_ms: number | null;
      refusal: null | {
        reason: "no_play_rights";
        title: string;
        artist: string;
      };
    }>,
    playlists: [] as Array<{
      remote_id: string;
      name: string;
      track_count: number;
    }>,
    playlistDetail: null as null | {
      remote_id: string;
      name: string;
      tracks: Array<{
        source_id: "netease";
        remote_track_id: string;
        title: string;
        artist: string;
        album: string | null;
        duration_ms: number | null;
        refusal: null | {
          reason: "no_play_rights";
          title: string;
          artist: string;
        };
      }>;
    },
    searchResults: [] as never[],
    importFailures: [] as Array<{
      remote_track_id: string;
      title: string;
      artist: string;
      reason: "refusal";
    }>,
    loadSession: vi.fn(),
    openSignIn: vi.fn(),
    completeSignIn: vi.fn(),
    signOut: vi.fn(),
    loadLiked: vi.fn(),
    loadPlaylists: vi.fn(),
    openPlaylist: vi.fn(),
    search: vi.fn(),
    importTracks: vi.fn(),
  },
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: { name?: string }) =>
      opts?.name ? `${key}:${opts.name}` : key,
  }),
}));

vi.mock("@/stores/catalog-store", () => ({
  useCatalogStore: (selector: (state: typeof mockState) => unknown) =>
    selector(mockState),
}));

describe("NeteasePanel sign-in", () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    (
      globalThis as typeof globalThis & {
        IS_REACT_ACT_ENVIRONMENT?: boolean;
      }
    ).IS_REACT_ACT_ENVIRONMENT = true;
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
    user = userEvent.setup();
    mockState.session = null;
    mockState.liked = [];
    mockState.playlists = [];
    mockState.playlistDetail = null;
    mockState.importFailures = [];
    mockState.signOut.mockReset();
    mockState.loadSession.mockReset();
  });

  afterEach(() => {
    act(() => root.unmount());
    container.remove();
  });

  test("opens official sign-in and checks completion with keyboard controls", async () => {
    await act(async () => {
      root.render(<NeteasePanel />);
    });
    expect(container.querySelector("input")).toBeNull();
    const connect = container.querySelector<HTMLButtonElement>(
      "[data-testid=netease-connect]",
    )!;
    const check = container.querySelector<HTMLButtonElement>(
      "[data-testid=netease-check-signin]",
    )!;
    connect.focus();
    await user.keyboard("{Enter}");
    expect(mockState.openSignIn).toHaveBeenCalled();
    await user.tab();
    expect(document.activeElement).toBe(check);
    await user.keyboard("{Enter}");
    expect(mockState.completeSignIn).toHaveBeenCalled();
  });

  test("signed-in name and Sign out are visible; expired sessions return to sign-in", async () => {
    mockState.session = {
      source_id: "netease",
      signed_in: true,
      display_name: "Ada",
      expired: false,
    };
    await act(async () => {
      root.render(<NeteasePanel />);
    });
    expect(
      container.querySelector("[data-testid='netease-signed-in']")?.textContent,
    ).toContain("Ada");
    expect(container.textContent).toContain("catalog.netease.signOut");
    expect(
      container.querySelector("[data-testid='netease-signin']"),
    ).toBeNull();

    mockState.session = {
      source_id: "netease",
      signed_in: true,
      display_name: "Ada",
      expired: true,
    };
    await act(async () => {
      root.render(<NeteasePanel />);
    });
    expect(
      container.querySelector("[data-testid='netease-signin']"),
    ).not.toBeNull();
    expect(
      container.querySelector("[data-testid='netease-session-expired']")
        ?.textContent,
    ).toBe("catalog.netease.sessionExpired");
    expect(
      container.querySelector("[data-testid='netease-signed-in']"),
    ).toBeNull();
  });

  test("signed-in browse can load, search, and import tracks", async () => {
    const track = {
      source_id: "netease" as const,
      remote_track_id: "1",
      title: "Night",
      artist: "Ada",
      album: null,
      duration_ms: 1000,
      refusal: null,
    };
    mockState.session = {
      source_id: "netease",
      signed_in: true,
      display_name: "Ada",
      expired: false,
    };
    mockState.liked = [
      track,
      {
        ...track,
        remote_track_id: "9",
        title: "Grey",
        refusal: {
          reason: "no_play_rights",
          title: "Grey",
          artist: "C",
        },
      },
    ];
    mockState.playlists = [{ remote_id: "pl-1", name: "Set", track_count: 1 }];
    mockState.playlistDetail = {
      remote_id: "pl-1",
      name: "Set",
      tracks: [track],
    };
    mockState.importFailures = [
      {
        remote_track_id: "9",
        title: "Grey",
        artist: "C",
        reason: "refusal",
      },
    ];

    await act(async () => {
      root.render(<NeteasePanel />);
    });

    await user.click(
      Array.from(container.querySelectorAll("button")).find((button) =>
        button.textContent?.includes("catalog.netease.signOut"),
      )!,
    );
    expect(mockState.signOut).toHaveBeenCalled();

    await user.click(
      Array.from(container.querySelectorAll("button")).find((button) =>
        button.textContent?.includes("catalog.netease.liked"),
      )!,
    );
    expect(mockState.loadLiked).toHaveBeenCalled();

    await user.click(
      Array.from(container.querySelectorAll("button")).find((button) =>
        button.textContent?.includes("catalog.netease.playlists"),
      )!,
    );
    expect(mockState.loadPlaylists).toHaveBeenCalled();

    await user.click(
      Array.from(container.querySelectorAll("button")).find((button) =>
        button.textContent?.includes("Set"),
      )!,
    );
    expect(mockState.openPlaylist).toHaveBeenCalledWith("pl-1");

    const search = container.querySelector(
      "[aria-label='catalog.netease.search']",
    ) as HTMLInputElement;
    await user.type(search, "night");
    await user.click(
      Array.from(container.querySelectorAll("button")).find((button) =>
        button.textContent?.includes("catalog.netease.search"),
      )!,
    );
    expect(mockState.search).toHaveBeenCalledWith("night");

    await user.click(
      Array.from(container.querySelectorAll("button")).find((button) =>
        button.textContent?.includes("catalog.netease.importPlaylist"),
      )!,
    );
    expect(mockState.importTracks).toHaveBeenCalledWith(["1"], "pl-1");

    await user.click(
      Array.from(container.querySelectorAll("button")).find((button) =>
        button.textContent?.includes("catalog.netease.importTrack"),
      )!,
    );
    expect(mockState.importTracks).toHaveBeenCalledWith(["1"]);
    expect(container.textContent).toContain("Grey");
  });
});
