import { describe, expect, test, vi } from "vitest";
import { createCatalogStore } from "./catalog-store";
import { createMockBackend } from "@/lib/backend/mock-backend";

vi.mock("@/lib/errors", () => ({
  notifyError: vi.fn(),
}));

describe("catalog store QR status", () => {
  test("startQr records waiting and pollQr can move to scanned then signed in", async () => {
    const backend = createMockBackend({
      overrides: {
        catalog: {
          startStreamingQrSignin: async () => ({
            key: "unikey",
            login_url: "https://music.163.com/login?codekey=unikey",
            qr_svg: "<svg></svg>",
          }),
          pollStreamingQrSignin: vi
            .fn()
            .mockResolvedValueOnce({ status: "scanned", session: null })
            .mockResolvedValueOnce({
              status: "confirmed",
              session: {
                source_id: "netease",
                signed_in: true,
                display_name: "Ada",
                expired: false,
              },
            }),
        },
      },
    });
    const store = createCatalogStore(backend);
    await store.getState().startQr();
    expect(store.getState().qr?.key).toBe("unikey");
    expect(store.getState().qrStatus).toBe("waiting");

    await store.getState().pollQr();
    expect(store.getState().qrStatus).toBe("scanned");
    expect(store.getState().session).toBeNull();

    await store.getState().pollQr();
    expect(store.getState().session?.display_name).toBe("Ada");
    expect(store.getState().qr).toBeNull();
    expect(store.getState().qrStatus).toBeNull();
  });

  test("password sign-in is forgotten by the store after the command returns", async () => {
    const signIn = vi.fn().mockResolvedValue({
      source_id: "netease",
      signed_in: true,
      display_name: "Ada",
      expired: false,
    });
    const backend = createMockBackend({
      overrides: {
        catalog: {
          signInStreamingSource: signIn,
        },
      },
    });
    const store = createCatalogStore(backend);
    await store.getState().signInPassword("email", "a@b.c", "once-only");
    expect(signIn).toHaveBeenCalledWith(
      "netease",
      "email",
      "a@b.c",
      "once-only",
      undefined,
    );
    expect(store.getState().session?.signed_in).toBe(true);
    expect(JSON.stringify(store.getState())).not.toContain("once-only");
  });
});
