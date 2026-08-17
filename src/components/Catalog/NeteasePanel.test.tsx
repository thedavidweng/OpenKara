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
    liked: [] as never[],
    playlists: [] as never[],
    playlistDetail: null,
    searchResults: [] as never[],
    importFailures: [] as never[],
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
});
