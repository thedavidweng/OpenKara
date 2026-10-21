// SPDX-License-Identifier: AGPL-3.0-only
import { readFile } from "node:fs/promises";
import { createInterface } from "node:readline";
const manifest = JSON.parse(
  await readFile(new URL("manifest.json", import.meta.url), "utf8"),
);
const web = manifest.authentication.flow === "web";
const credentials = { opaque_session: `${manifest.source_id}-verified` };
const account = {
  id: "same-account-id",
  display_name: `${manifest.name} singer`,
};
let session = { state: "signed_out", account: null };
let negotiated = false;
let challenge = null;
let polls = 0;
let generation = 0;
const cursors = new Set();
const tracks = ["1", "3"].map((id) => ({
  id,
  title: `${manifest.name} ${id}`,
  artists: [`${manifest.name} artist`],
  album: null,
  duration_ms: 1000,
  artwork: {
    url: "https://127.0.0.1/private-image.png",
    headers: {},
    expires_at: null,
  },
  refusal: null,
}));
const playlist = {
  id: "42",
  name: `${manifest.name} set`,
  description: null,
  artwork: null,
  track_count: 2,
};
const fail = (code) => {
  throw Object.assign(new Error(code), { code });
};
const signIn = () => {
  session = { state: "signed_in", account };
  return session;
};
function page(params) {
  if (params.cursor === null) {
    const cursor = `${manifest.source_id}:${generation++}`;
    cursors.add(cursor);
    return { items: tracks.slice(0, 1), next_cursor: cursor };
  }
  if (!cursors.delete(params.cursor)) fail("invalid_cursor");
  return { items: tracks.slice(1), next_cursor: null };
}
async function dispatch(method, params) {
  if (method === "handshake") {
    if (negotiated || !params.protocol_majors.includes(1))
      fail("incompatible_plugin");
    negotiated = true;
    return {
      protocol: { major: 1, minor: 0 },
      source_id: manifest.source_id,
      package_id: manifest.package_id,
      package_version: manifest.version,
    };
  }
  if (!negotiated) fail("handshake_required");
  if (method === "describe") return { ...manifest, session };
  if (method === "request.cancel") return { cancelled: false };
  if (method === "session.restore") {
    if (params.credentials?.opaque_session !== credentials.opaque_session)
      fail("session_expired");
    return signIn();
  }
  if (method === "auth.start") {
    challenge = `${manifest.source_id}:challenge:${generation++}`;
    polls = 0;
    return {
      challenge,
      expires_at: new Date(Date.now() + 60000).toISOString(),
      poll_after_ms: 1,
      presentation: web
        ? { kind: "web" }
        : {
            kind: "qr",
            content: "fixture:approve",
            text: "Deterministic fixture approval",
          },
    };
  }
  if (method === "auth.cancel") {
    challenge = null;
    return { state: "cancelled" };
  }
  if (method === "auth.sign_out") {
    challenge = null;
    session = { state: "signed_out", account: null };
    return session;
  }
  if (method === "auth.complete") {
    if (!web) fail("unsupported_operation");
    if (params.challenge !== challenge) fail("challenge_expired");
    if (params.credentials?.official_cookie !== "verified-cookie")
      fail("authentication_required");
    return { session: signIn(), credentials };
  }
  if (method === "auth.poll") {
    if (web) fail("unsupported_operation");
    if (params.challenge !== challenge) fail("challenge_expired");
    return ++polls < 2
      ? { state: "pending", poll_after_ms: 1 }
      : { state: "complete", session: signIn(), credentials };
  }
  const capability =
    method === "playlist.tracks"
      ? "playlist_tracks"
      : method === "tracks.get"
        ? "track_details"
        : method === "media.resolve"
          ? params.purpose
          : method;
  if (!manifest.capabilities.includes(capability))
    fail("unsupported_operation");
  if (
    manifest.requires_authentication.includes(method) &&
    session.state !== "signed_in"
  )
    fail("authentication_required");
  if (method === "search" || method === "liked_tracks") return page(params);
  if (method === "playlists") return { items: [playlist], next_cursor: null };
  if (method === "playlist.tracks") {
    if (params.playlist_id !== playlist.id) fail("not_found");
    return { playlist, ...page(params) };
  }
  if (method === "tracks.get")
    return {
      items: tracks.filter((track) => params.track_ids.includes(track.id)),
      missing_ids: [],
    };
  if (method === "media.resolve") {
    if (params.track_id === "refused")
      return {
        kind: "refusal",
        refusal: { reason: "trial_clip", message: "Fixture trial clip" },
      };
    const extension = "wav";
    return {
      kind: "media",
      media: {
        url: `https://raw.githubusercontent.com/thedavidweng/OpenKara/f439d2d94edff292b2f38b10bbe279a2e355e7dc/src-tauri/tests/fixtures/audio/fixture.${extension}${web ? "" : "?source=challenge"}`,
        headers: { "User-Agent": "OpenKara-Fixture" },
        expires_at: null,
        content_type: "audio/wav",
        file_extension: extension,
        size_bytes: null,
        duration_ms: 1000,
        bit_rate: null,
      },
    };
  }
  fail("unsupported_operation");
}
for await (const line of createInterface({
  input: process.stdin,
  crlfDelay: Infinity,
})) {
  const frame = JSON.parse(line);
  try {
    process.stdout.write(
      `${JSON.stringify({ id: frame.id, result: await dispatch(frame.method, frame.params) })}\n`,
    );
  } catch (error) {
    process.stdout.write(
      `${JSON.stringify({ id: frame.id, error: { code: error.code ?? "internal", message: "Fixture request failed", retryable: false } })}\n`,
    );
  }
}
