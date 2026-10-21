import { describe, expect, test } from "vitest";
import { createCatalogCommands } from "@/lib/tauri/catalog";
import { E2E_MOCK_DATA } from "./tauri-mock-data";
import { createTauriMock } from "./tauri-mock-impl";

function catalog() {
  return createCatalogCommands(createTauriMock(E2E_MOCK_DATA).internals.invoke);
}

describe("UI-only streaming source harness", () => {
  test("publishes caller-assigned source and job identity while start is pending", async () => {
    const backend = catalog();
    const completion = backend.startStreamingImport(
      "demo-web",
      ["shared:/track"],
      null,
      "ui-assigned-job",
    );
    expect(await backend.getStreamingImportStatus()).toMatchObject({
      source_id: "demo-web",
      job_id: "ui-assigned-job",
      phase: "running",
    });
    await backend.cancelStreamingImport();
    expect(await completion).toMatchObject({
      source_id: "demo-web",
      failed: [{ remote_track_id: "shared:/track", reason: "cancelled" }],
    });
    expect(await backend.getStreamingImportStatus()).toMatchObject({
      source_id: "demo-web",
      job_id: "ui-assigned-job",
      phase: "completed",
    });
  });

  test("reports installation progress and cancellation without changing installed version", async () => {
    const backend = catalog();
    const installation = backend.installStreamingSource("demo-web", "2.0.0");
    const rejected = expect(installation).rejects.toMatchObject({
      code: "streaming_cancelled",
    });
    expect((await backend.listStreamingSources())[0].state).toBe("installing");
    await backend.cancelStreamingSourceInstall("demo-web");
    await rejected;
    expect((await backend.listStreamingSources())[0].installed_version).toBe(
      "1.0.0",
    );
  });

  test("keeps native registry limited to YouTube", async () => {
    const mock = createTauriMock(E2E_MOCK_DATA);
    expect(await mock.internals.invoke("list_online_sources")).toEqual([
      {
        id: "youtube",
        kind: "video",
        enabled: false,
        capabilities: {
          sign_in: false,
          browse: false,
          import: false,
          resolve_video: false,
        },
      },
    ]);
  });

  test("keeps colliding remote ids separate and routes opaque pages by source", async () => {
    const backend = catalog();
    const sources = await backend.listStreamingSources();
    expect(sources.map((source) => source.source_id)).toEqual([
      "demo-web",
      "demo-plugin",
    ]);
    const first = await backend.listStreamingLikedTracks("demo-web");
    const other = await backend.searchStreamingSource("demo-plugin", "");
    expect(first.tracks[0].remote_track_id).toBe("shared:/track");
    expect(other.tracks[0].remote_track_id).toBe("shared:/track");
    expect(first.tracks[0].source_id).toBe("demo-web");
    expect(other.tracks[0].source_id).toBe("demo-plugin");
    expect(
      (await backend.listStreamingLikedTracks("demo-web", first.next_cursor))
        .tracks,
    ).toHaveLength(1);
    await expect(
      backend.searchStreamingSource("demo-plugin", "", first.next_cursor),
    ).rejects.toMatchObject({ code: "streaming_invalid_cursor" });
  });

  test("refuses removal of active auth and cancels it only with explicit consent", async () => {
    const backend = catalog();
    const auth = await backend.openStreamingSignIn("demo-plugin");
    expect(auth.presentation.kind).toBe("qr");
    await expect(
      backend.uninstallStreamingSource("demo-plugin", false, false),
    ).rejects.toMatchObject({ code: "streaming_work_active" });
    const removed = await backend.uninstallStreamingSource(
      "demo-plugin",
      true,
      false,
    );
    expect(removed.state).toBe("not_installed");
    expect(
      (await backend.pollStreamingSignIn("demo-plugin", auth.challenge)).state,
    ).toBe("cancelled");
  });

  test("keeps source sessions independent and honors credential removal choice", async () => {
    const backend = catalog();
    const auth = await backend.openStreamingSignIn("demo-web");
    expect(auth.presentation).toEqual({ kind: "web" });
    expect(
      (await backend.pollStreamingSignIn("demo-web", auth.challenge)).state,
    ).toBe("pending");
    expect(
      (await backend.pollStreamingSignIn("demo-web", auth.challenge)).state,
    ).toBe("complete");
    expect((await backend.getStreamingSession("demo-plugin")).signed_in).toBe(
      false,
    );
    await backend.uninstallStreamingSource("demo-web", false, false);
    expect((await backend.getStreamingSession("demo-web")).signed_in).toBe(
      true,
    );
    await backend.installStreamingSource("demo-web");
    await backend.uninstallStreamingSource("demo-web", false, true);
    expect((await backend.getStreamingSession("demo-web")).signed_in).toBe(
      false,
    );
  });
});
