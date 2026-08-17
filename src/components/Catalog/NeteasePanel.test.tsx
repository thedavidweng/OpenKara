// @vitest-environment jsdom
import userEvent from "@testing-library/user-event";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { NeteasePanel } from "./NeteasePanel";
import type {
  StreamingQrChallenge,
  StreamingQrStatus,
  StreamingSessionSnapshot,
} from "@/types/ipc";

const qrChallenge: StreamingQrChallenge = {
  key: "unikey",
  login_url: "https://music.163.com/login?codekey=unikey",
  qr_svg: '<svg data-testid="netease-qr-svg"></svg>',
};

const { mockState } = vi.hoisted(() => ({
  mockState: {
    session: null as StreamingSessionSnapshot | null,
    qr: null as StreamingQrChallenge | null,
    qrStatus: null as StreamingQrStatus | null,
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
    startQr: vi.fn(),
    pollQr: vi.fn(),
    signInPassword: vi.fn(),
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
    mockState.qr = qrChallenge;
    mockState.qrStatus = "waiting";
    mockState.liked = [];
    mockState.playlists = [];
    mockState.playlistDetail = null;
    mockState.importFailures = [];
    mockState.startQr.mockReset();
    mockState.pollQr.mockReset();
    mockState.signInPassword.mockReset();
    mockState.signOut.mockReset();
    mockState.loadSession.mockReset();
  });

  afterEach(() => {
    act(() => root.unmount());
    container.remove();
  });

  test("QR is the default path and shows waiting, scanned, and expired states", async () => {
    await act(async () => {
      root.render(<NeteasePanel />);
    });

    const signIn = container.querySelector("[data-testid='netease-signin']");
    expect(signIn?.getAttribute("data-mode")).toBe("qr");
    expect(
      container.querySelector("[data-testid='netease-qr']"),
    ).not.toBeNull();
    expect(
      container
        .querySelector("[data-testid='netease-qr']")
        ?.getAttribute("data-login-url"),
    ).toBe("https://music.163.com/login?codekey=unikey");
    expect(
      container.querySelector("[data-testid='netease-phone-form']"),
    ).toBeNull();
    expect(
      container.querySelector("[data-testid='netease-email-form']"),
    ).toBeNull();

    const status = container.querySelector("[data-testid='netease-qr-status']");
    expect(status?.getAttribute("data-status")).toBe("waiting");
    expect(status?.textContent).toBe("catalog.netease.qrHint");

    mockState.qrStatus = "scanned";
    await act(async () => {
      root.render(<NeteasePanel />);
    });
    expect(
      container
        .querySelector("[data-testid='netease-qr-status']")
        ?.getAttribute("data-status"),
    ).toBe("scanned");
    expect(
      container.querySelector("[data-testid='netease-qr-status']")?.textContent,
    ).toBe("catalog.netease.scanned");

    mockState.qrStatus = "expired";
    await act(async () => {
      root.render(<NeteasePanel />);
    });
    expect(
      container
        .querySelector("[data-testid='netease-qr-status']")
        ?.getAttribute("data-status"),
    ).toBe("expired");
    expect(
      container.querySelector("[data-testid='netease-qr-status']")?.textContent,
    ).toBe("catalog.netease.qrExpired");
    expect(container.textContent).toContain("catalog.netease.refreshQr");
  });

  test("phone and email modes are switchable and the password field is a password input", async () => {
    await act(async () => {
      root.render(<NeteasePanel />);
    });

    await user.click(
      container.querySelector("[data-testid='netease-use-phone']")!,
    );

    const phoneForm = container.querySelector(
      "[data-testid='netease-phone-form']",
    );
    expect(phoneForm).not.toBeNull();
    expect(container.querySelector("[data-testid='netease-qr']")).toBeNull();
    const phonePassword = container.querySelector<HTMLInputElement>(
      "#netease-phone-password",
    );
    expect(phonePassword?.type).toBe("password");
    expect(container.textContent).toContain("catalog.netease.passwordNotice");

    await user.click(
      container.querySelector("[data-testid='netease-use-email']")!,
    );
    expect(
      container.querySelector("[data-testid='netease-email-form']"),
    ).not.toBeNull();
    expect(
      container.querySelector<HTMLInputElement>("#netease-email-password")
        ?.type,
    ).toBe("password");
    expect(
      container.querySelector("[data-testid='netease-use-qr']"),
    ).not.toBeNull();
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

  test("QR starts itself, refreshes when expired, and password forms sign in once", async () => {
    mockState.qr = null;
    mockState.qrStatus = null;
    await act(async () => {
      root.render(<NeteasePanel />);
    });
    expect(mockState.startQr).toHaveBeenCalled();

    mockState.qr = qrChallenge;
    mockState.qrStatus = "expired";
    await act(async () => {
      root.render(<NeteasePanel />);
    });
    await user.click(
      Array.from(container.querySelectorAll("button")).find((button) =>
        button.textContent?.includes("catalog.netease.refreshQr"),
      )!,
    );
    expect(mockState.startQr).toHaveBeenCalled();

    await user.click(
      container.querySelector("[data-testid='netease-use-phone']")!,
    );
    const phone = container.querySelector(
      "#netease-phone-number",
    ) as HTMLInputElement;
    const phonePassword = container.querySelector(
      "#netease-phone-password",
    ) as HTMLInputElement;
    await user.type(phone, "138 0013 8000");
    await user.type(phonePassword, "secret");
    await user.click(
      container.querySelector(
        "[data-testid='netease-phone-form'] button[type='submit']",
      )!,
    );
    expect(mockState.signInPassword).toHaveBeenCalledWith(
      "phone",
      "13800138000",
      "secret",
      "86",
    );

    await user.click(
      container.querySelector("[data-testid='netease-use-email']")!,
    );
    await user.type(
      container.querySelector("#netease-email") as HTMLInputElement,
      "ada@example.com",
    );
    await user.type(
      container.querySelector("#netease-email-password") as HTMLInputElement,
      "secret",
    );
    await user.click(
      container.querySelector(
        "[data-testid='netease-email-form'] button[type='submit']",
      )!,
    );
    expect(mockState.signInPassword).toHaveBeenCalledWith(
      "email",
      "ada@example.com",
      "secret",
    );

    mockState.qr = null;
    await user.click(
      container.querySelector("[data-testid='netease-use-qr']")!,
    );
    expect(mockState.startQr).toHaveBeenCalled();
  });
});
