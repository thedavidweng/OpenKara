// SPDX-License-Identifier: AGPL-3.0-only
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createInterface } from "node:readline";
import { once } from "node:events";
import { fileURLToPath } from "node:url";

const child = spawn(
  process.execPath,
  [fileURLToPath(new URL("worker.mjs", import.meta.url))],
  {
    stdio: ["pipe", "pipe", "inherit"],
  },
);
const closed = once(child, "close");
const scenario = process.env.OPENKARA_FIXTURE_SCENARIO ?? "normal";
const pending = new Map();
const events = [];
let sequence = 0;
let failure;
const watchdog = setTimeout(() => {
  failure = new Error("Fixture smoke timed out");
  child.kill();
}, 10000);
const reader = createInterface({ input: child.stdout });
reader.on("line", (line) => {
  try {
    const frame = JSON.parse(line);
    if (frame.event) events.push(frame);
    else {
      assert(pending.has(frame.id), "Unexpected or duplicate response");
      pending.get(frame.id)(frame);
      pending.delete(frame.id);
    }
  } catch (error) {
    failure = error;
    child.kill();
  }
});
child.on("exit", () => {
  for (const resolve of pending.values())
    resolve({ error: { code: "process_exited" } });
  pending.clear();
});
function send(method, params = {}) {
  const id = `smoke-${++sequence}`;
  const response = new Promise((resolve) => pending.set(id, resolve));
  child.stdin.write(`${JSON.stringify({ id, method, params })}\n`);
  return { id, response };
}
async function call(method, params = {}) {
  const frame = await send(method, params).response;
  assert.equal(frame.error, undefined, JSON.stringify(frame.error));
  return frame.result;
}
async function error(method, params, code) {
  assert.equal((await send(method, params).response).error?.code, code);
}
async function smoke() {
  await error(
    "session.restore",
    { credentials: { fixture_session: "valid" } },
    "handshake_required",
  );
  await error(
    "handshake",
    { protocol_majors: [99], host_version: "0.17.0" },
    "incompatible_plugin",
  );
  // A rejected handshake has not established a session; this smoke retries in
  // the same process to inspect the fixture. A production host terminates it.
  const hello = await call("handshake", {
    protocol_majors: [1],
    host_version: "0.17.0",
  });
  assert.equal(hello.source_id, "fixture");
  assert.equal(hello.protocol.major, 1);
  const description = await call("describe");
  assert.deepEqual(description.capabilities, [
    "playlists",
    "playlist_tracks",
    "track_details",
    "import",
  ]);
  assert.equal(description.session.state, "signed_out");
  await error("playlists", { cursor: null }, "authentication_required");
  await error(
    "search",
    { query: "tone", cursor: null },
    "unsupported_operation",
  );
  const first = await call("auth.start");
  assert.equal(first.presentation.kind, scenario === "text" ? "text" : "qr");
  assert.equal(
    (await call("auth.cancel", { challenge: first.challenge })).state,
    "cancelled",
  );
  assert.equal(
    (await call("auth.poll", { challenge: first.challenge })).state,
    "cancelled",
  );
  const auth = await call("auth.start");
  if (scenario === "expired_challenge") {
    assert.equal(
      (await call("auth.poll", { challenge: auth.challenge })).state,
      "expired",
    );
    console.log("PASS real stdio: expired challenge remains unauthenticated");
    return;
  }
  assert.equal(
    (await call("auth.poll", { challenge: auth.challenge })).state,
    "pending",
  );
  const signedIn = await call("auth.poll", { challenge: auth.challenge });
  assert.equal(signedIn.state, "complete");
  assert.equal(signedIn.session.account.display_name, "Fixture Singer");
  if (scenario === "network") {
    await error("playlists", { cursor: null }, "network");
    assert.equal((await call("describe")).session.state, "signed_in");
    console.log(
      "PASS real stdio: network failure is distinct from session expiry and refusal",
    );
    return;
  }
  const playlists = await call("playlists", { cursor: null });
  assert.equal(playlists.items[0].id, "42");
  const head = await call("playlist.tracks", {
    playlist_id: "42",
    cursor: null,
  });
  assert.deepEqual(
    head.items.map((track) => track.id),
    ["1"],
  );
  assert.equal(typeof head.next_cursor, "string");
  const rest = await call("playlist.tracks", {
    playlist_id: "42",
    cursor: head.next_cursor,
  });
  assert.deepEqual(
    rest.items.map((track) => track.id),
    ["2", "3"],
  );
  assert.equal(rest.next_cursor, null);
  await error(
    "playlist.tracks",
    { playlist_id: "42", cursor: head.next_cursor },
    "invalid_cursor",
  );
  const details = await call("tracks.get", { track_ids: ["3", "absent"] });
  assert.equal(details.items[0].title, "Fixture Night Tone");
  assert.deepEqual(details.missing_ids, ["absent"]);
  const refusal = await call("media.resolve", {
    track_id: "2",
    purpose: "import",
  });
  assert.equal(refusal.kind, "refusal");
  assert.equal(refusal.refusal.reason, "trial_clip");
  const media = await call("media.resolve", {
    track_id: "1",
    purpose: "import",
  });
  assert.equal(
    media.media.url,
    "https://streaming-fixture.openkara.test:9443/tone-1.wav",
  );
  assert.equal(media.media.size_bytes, 16044);
  const slow = send("media.resolve", { track_id: "3", purpose: "import" });
  assert.equal(
    (await call("request.cancel", { request_id: slow.id })).cancelled,
    true,
  );
  assert.equal((await slow.response).error.code, "cancelled");
  assert(
    events.some(
      (event) => event.request_id === slow.id && event.data.phase === "resolve",
    ),
  );
  assert.equal((await call("auth.sign_out")).state, "signed_out");
  assert.equal(
    (await call("session.restore", { credentials: signedIn.credentials }))
      .state,
    "signed_in",
  );
  await error(
    "session.restore",
    { credentials: { fixture_session: "expired" } },
    "session_expired",
  );
  assert.equal((await call("describe")).session.state, "expired");
  await error("playlists", { cursor: null }, "session_expired");
  console.log(
    "PASS real stdio: handshake, capabilities, QR auth/cancel, opaque pagination, details, refusal, media, progress/cancel, sign-out/restore/expiry",
  );
}
let exitCode;
try {
  await smoke();
} finally {
  child.stdin.end();
  [exitCode] = await closed;
  clearTimeout(watchdog);
}
if (failure) throw failure;
assert.equal(exitCode, 0);
