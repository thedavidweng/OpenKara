// SPDX-License-Identifier: AGPL-3.0-only
import { readFile } from "node:fs/promises";
import { setTimeout as delay } from "node:timers/promises";

const manifest = JSON.parse(
  await readFile(new URL("manifest.json", import.meta.url), "utf8"),
);
const origin = manifest.media_origins[0];
const scenario = process.env.OPENKARA_FIXTURE_SCENARIO ?? "normal";
if (!["normal", "text", "expired_challenge", "network"].includes(scenario)) {
  process.stderr.write("Unknown fixture scenario\n");
  process.exit(1);
}
const refusal = {
  reason: "trial_clip",
  message: "Only a trial clip is permitted for this fixture track.",
};
const artwork = { url: `${origin}/artwork.png`, headers: {}, expires_at: null };
const tracks = [
  {
    id: "1",
    title: "Fixture Morning Tone",
    artists: ["Fixture Ensemble"],
    album: "Generated Signals",
    duration_ms: 1000,
    artwork,
    refusal: null,
  },
  {
    id: "2",
    title: "Fixture Refused Tone",
    artists: ["Fixture Ensemble"],
    album: "Generated Signals",
    duration_ms: 1000,
    artwork,
    refusal,
  },
  {
    id: "3",
    title: "Fixture Night Tone",
    artists: ["Fixture Soloist"],
    album: "Generated Signals",
    duration_ms: 1000,
    artwork,
    refusal: null,
  },
];
const playlist = {
  id: "42",
  name: "Fixture Signals",
  description: "Three generated tracks; one deliberate refusal.",
  artwork,
  track_count: 3,
};
const supported = new Set([
  "describe",
  "session.restore",
  "auth.start",
  "auth.poll",
  "auth.cancel",
  "auth.sign_out",
  "playlists",
  "playlist.tracks",
  "tracks.get",
  "media.resolve",
]);
const seen = new Set();
const active = new Map();
const cursors = new Map();
const challenges = new Map();
let session = { state: "signed_out", account: null };
let negotiated = false;
let tokenSequence = 0;
let output = Promise.resolve();
let fatal = false;
const account = { id: "fixture-account", display_name: "Fixture Singer" };
const credentials = { fixture_session: "fixture-account-v1" };
function fault(code, message = code.replaceAll("_", " ")) {
  return Object.assign(new Error(message), { code });
}
function emit(frame) {
  output = output.then(
    () =>
      new Promise((resolve, reject) => {
        process.stdout.write(`${JSON.stringify(frame)}\n`, (error) =>
          error ? reject(error) : resolve(),
        );
      }),
  );
  return output;
}
function object(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}
function text(value) {
  return typeof value === "string" && value.length > 0;
}
function requireField(condition) {
  if (!condition) throw fault("invalid_request");
}
function clearSession(state = "signed_out") {
  session = { state, account: null };
  cursors.clear();
  challenges.clear();
}
function signIn() {
  session = { state: "signed_in", account };
  cursors.clear();
  return session;
}
function cursorPage(method, params) {
  requireField(params.cursor === null || text(params.cursor));
  if (params.cursor !== null) {
    const cursor = cursors.get(params.cursor);
    if (
      !cursor ||
      cursor.method !== method ||
      cursor.playlist_id !== params.playlist_id
    )
      throw fault("invalid_cursor");
    cursors.delete(params.cursor);
    return { items: tracks.slice(1), next_cursor: null };
  }
  if (method === "playlists") return { items: [playlist], next_cursor: null };
  const cursor = `fixture-page-${++tokenSequence}-continue`;
  cursors.set(cursor, { method, playlist_id: params.playlist_id });
  return { items: tracks.slice(0, 1), next_cursor: cursor };
}
function dispatch(method, params, id, signal) {
  if (method === "handshake") {
    requireField(
      !negotiated &&
        Array.isArray(params.protocol_majors) &&
        params.protocol_majors.every(Number.isInteger) &&
        text(params.host_version),
    );
    if (!params.protocol_majors.includes(1)) throw fault("incompatible_plugin");
    negotiated = true;
    return {
      protocol: { major: 1, minor: 0 },
      package_id: manifest.package_id,
      source_id: manifest.source_id,
      package_version: manifest.version,
    };
  }
  if (!negotiated) throw fault("handshake_required");
  if (method === "request.cancel") {
    requireField(text(params.request_id));
    const target = active.get(params.request_id);
    if (!target || target.method === "request.cancel")
      return { cancelled: false };
    target.controller.abort();
    return { cancelled: true };
  }
  if (
    !supported.has(method) ||
    (method === "media.resolve" && params.purpose !== "import")
  )
    throw fault("unsupported_operation");
  if (method === "describe")
    return {
      name: manifest.name,
      description: manifest.description,
      capabilities: manifest.capabilities,
      authentication: manifest.authentication,
      requires_authentication: manifest.requires_authentication,
      session,
    };
  if (method === "session.restore") {
    requireField(
      object(params.credentials) &&
        Object.values(params.credentials).every(
          (value) => typeof value === "string",
        ),
    );
    clearSession();
    if (params.credentials.fixture_session !== credentials.fixture_session) {
      clearSession("expired");
      throw fault("session_expired");
    }
    return signIn();
  }
  if (method === "auth.sign_out") {
    for (const [otherId, request] of active) {
      if (
        otherId !== id &&
        manifest.requires_authentication.includes(request.method)
      )
        request.controller.abort();
    }
    clearSession();
    return session;
  }
  if (method === "auth.start") {
    challenges.clear();
    const challenge = `fixture-challenge-${++tokenSequence}`;
    const expires =
      Date.now() + (scenario === "expired_challenge" ? -1 : 60000);
    challenges.set(challenge, { expires, polls: 0, state: "pending" });
    return {
      challenge,
      expires_at: new Date(expires).toISOString(),
      poll_after_ms: 1,
      presentation:
        scenario === "text"
          ? {
              kind: "text",
              text: "Fixture sign-in: poll twice to approve the test account. No real credentials.",
            }
          : {
              kind: "qr",
              content: `openkara-fixture:approve:${challenge}`,
              text: "Test QR: the second poll approves Fixture Singer. Do not scan with a real music app.",
            },
    };
  }
  if (method === "auth.poll" || method === "auth.cancel") {
    requireField(text(params.challenge));
    const challenge = challenges.get(params.challenge);
    if (!challenge) throw fault("challenge_expired");
    if (method === "auth.cancel") challenge.state = "cancelled";
    if (challenge.state === "pending" && Date.now() >= challenge.expires)
      challenge.state = "expired";
    if (challenge.state === "pending" && ++challenge.polls >= 2) {
      challenge.state = "complete";
      signIn();
    }
    if (challenge.state === "complete")
      return { state: "complete", session, credentials };
    if (challenge.state === "pending")
      return { state: "pending", poll_after_ms: 1 };
    return { state: challenge.state };
  }
  if (session.state !== "signed_in")
    throw fault(
      session.state === "expired"
        ? "session_expired"
        : "authentication_required",
    );
  if (scenario === "network")
    throw fault("network", "Fixture upstream connection failed");
  if (method === "playlists") return cursorPage(method, params);
  if (method === "playlist.tracks") {
    requireField(text(params.playlist_id));
    if (params.playlist_id !== playlist.id) throw fault("not_found");
    return { playlist, ...cursorPage(method, params) };
  }
  if (method === "tracks.get") {
    requireField(
      Array.isArray(params.track_ids) &&
        params.track_ids.length > 0 &&
        params.track_ids.length <= 100 &&
        params.track_ids.every(text),
    );
    return {
      items: params.track_ids.flatMap((trackId) =>
        tracks.filter((track) => track.id === trackId),
      ),
      missing_ids: params.track_ids.filter(
        (trackId) => !tracks.some((track) => track.id === trackId),
      ),
    };
  }
  requireField(text(params.track_id));
  const track = tracks.find((item) => item.id === params.track_id);
  if (!track) throw fault("not_found");
  if (track.refusal) return { kind: "refusal", refusal: track.refusal };
  return resolveMedia(track, id, signal);
}
async function resolveMedia(track, id, signal) {
  await emit({
    event: "progress",
    request_id: id,
    data: { phase: "resolve", completed: 0, total: 1 },
  });
  await delay(track.id === "3" ? 250 : 5, undefined, { signal });
  return {
    kind: "media",
    media: {
      url: `${origin}/tone-${track.id}.wav`,
      headers: {},
      expires_at: null,
      content_type: "audio/wav",
      file_extension: "wav",
      size_bytes: 16044,
      duration_ms: 1000,
      bit_rate: 128000,
    },
  };
}
function stopProtocol() {
  fatal = true;
  for (const request of active.values()) request.controller.abort();
  process.stderr.write("Invalid fixture protocol frame\n");
  process.exitCode = 1;
  process.stdin.destroy();
}
function accept(line) {
  if (fatal) return;
  let frame;
  try {
    frame = JSON.parse(line);
    if (
      !object(frame) ||
      !text(frame.id) ||
      frame.id.length > 128 ||
      seen.has(frame.id) ||
      !text(frame.method) ||
      !object(frame.params)
    )
      throw new Error();
  } catch {
    stopProtocol();
    return;
  }
  seen.add(frame.id);
  if (
    active.size >= 16 &&
    !["request.cancel", "auth.cancel", "auth.sign_out"].includes(frame.method)
  ) {
    void emit({
      id: frame.id,
      error: {
        code: "busy",
        message: "Fixture request limit reached",
        retryable: true,
      },
    });
    return;
  }
  const controller = new AbortController();
  const request = { method: frame.method, controller };
  active.set(frame.id, request);
  request.promise = (async () => {
    try {
      const dispatched = dispatch(
        frame.method,
        frame.params,
        frame.id,
        controller.signal,
      );
      const result =
        dispatched instanceof Promise ? await dispatched : dispatched;
      if (controller.signal.aborted) throw fault("cancelled");
      // Commit the terminal outcome before accepting a racing cancellation.
      active.delete(frame.id);
      if (!fatal) await emit({ id: frame.id, result });
    } catch (error) {
      active.delete(frame.id);
      const code = controller.signal.aborted
        ? "cancelled"
        : (error.code ?? "internal");
      if (!fatal)
        await emit({
          id: frame.id,
          error: {
            code,
            message: error.code ? error.message : code.replaceAll("_", " "),
            retryable: ["network", "busy"].includes(code),
          },
        });
    }
  })();
}
let buffered = Buffer.alloc(0);
for await (const chunk of process.stdin) {
  buffered = Buffer.concat([buffered, chunk]);
  let newline;
  while ((newline = buffered.indexOf(10)) !== -1) {
    if (newline + 1 > 1048576) {
      stopProtocol();
      break;
    }
    let line = buffered.subarray(0, newline);
    if (line.at(-1) === 13) line = line.subarray(0, -1);
    buffered = buffered.subarray(newline + 1);
    try {
      accept(
        new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(line),
      );
    } catch {
      stopProtocol();
    }
    if (fatal) break;
  }
  if (buffered.length >= 1048576) stopProtocol();
  if (fatal) break;
}
if (buffered.length && !fatal) stopProtocol();
const remaining = [...active.values()];
for (const request of remaining) request.controller.abort();
await Promise.all(remaining.map((request) => request.promise));
await output;
