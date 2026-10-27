// SPDX-License-Identifier: AGPL-3.0-only
// Deterministic Streaming Source fixture for the embedded engine: protocol
// major 2 on the openkara-js runtime, as a single ES module. The host test
// support replaces the placeholder on the next line with the package manifest
// JSON and serves the result as a Source Registry package; the sandboxed
// engine has no filesystem to read a manifest from.
const manifest = __MANIFEST__;

const api = manifest.resource_origins[0];
const web = manifest.authentication.flow === "web";
const account = {
  id: "same-account-id",
  display_name: `${manifest.name} singer`,
};
const credentials = { opaque_session: `${manifest.source_id}-verified` };
const trialClip = {
  reason: "trial_clip",
  message: "Only a trial clip is permitted for this fixture track.",
};
// Deliberately unreachable artwork: the host must refuse the private address
// and the catalog must treat the cover as absent.
const artwork = {
  url: "https://127.0.0.1/private-image.png",
  headers: {},
  expires_at: null,
};
// Track and playlist ids deliberately collide with the ids other sources use.
const tracks = ["1", "2", "3"].map((id) => ({
  id,
  title: `${manifest.name} ${id}`,
  artists: [`${manifest.name} artist`],
  album: "Generated Signals",
  duration_ms: 1000,
  artwork,
  refusal: id === "2" ? trialClip : null,
}));
const playlist = {
  id: "42",
  name: `${manifest.name} set`,
  description: "Three generated tracks; one deliberate refusal.",
  artwork: null,
  track_count: tracks.length,
};

let session = { state: "signed_out", account: null };
let challenge = null;
let sequence = 0;
const cursors = new Map();

function fault(code, message) {
  return Object.assign(new Error(message ?? code.split("_").join(" ")), {
    code,
  });
}

function signedIn() {
  session = { state: "signed_in", account };
  openkara.storage.set("account_id", account.id);
  return session;
}

function requireSession(method) {
  if (!manifest.requires_authentication.includes(method)) return;
  if (session.state === "expired") throw fault("session_expired");
  if (session.state !== "signed_in") throw fault("authentication_required");
}

// Continuation cursors are opaque, single-use, and bound to this engine run:
// another source's cursor, or a reused one, is invalid.
function page(params, items) {
  if (params.cursor === null || params.cursor === undefined) {
    if (items.length <= 1) return { items, next_cursor: null };
    const cursor = `${manifest.source_id}:${++sequence}`;
    cursors.set(cursor, items.slice(1));
    return { items: items.slice(0, 1), next_cursor: cursor };
  }
  const rest = cursors.get(params.cursor);
  if (rest === undefined) throw fault("invalid_cursor");
  cursors.delete(params.cursor);
  return { items: rest, next_cursor: null };
}

function describe(params) {
  // The host offers its protocol majors on the compatibility describe that
  // gates installation; plain describes carry no offer.
  const offered = params && params.protocol_majors;
  if (
    offered !== undefined &&
    (!Array.isArray(offered) || !offered.includes(2))
  ) {
    throw fault("incompatible_plugin");
  }
  return {
    protocol: { major: 2, minor: 0 },
    source_id: manifest.source_id,
    package_id: manifest.package_id,
    package_version: manifest.version,
    name: manifest.name,
    description: manifest.description,
    capabilities: manifest.capabilities,
    authentication: manifest.authentication,
    requires_authentication: manifest.requires_authentication,
    session,
    storage_keys: openkara.storage.keys(),
  };
}

function restore(params) {
  if (
    !params.credentials ||
    params.credentials.opaque_session !== credentials.opaque_session
  ) {
    session = { state: "expired", account: null };
    throw fault("session_expired");
  }
  return signedIn();
}

function startAuth(params) {
  const id = `${manifest.source_id}:challenge:${++sequence}`;
  challenge = { id, polls: 0 };
  let presentation;
  if (web) {
    presentation = { kind: "web" };
  } else {
    const declared = manifest.authentication.presentations;
    const requested = params && params.presentation;
    const kind = declared.includes(requested) ? requested : declared[0];
    presentation =
      kind === "text"
        ? {
            kind,
            text: "Fixture sign-in: poll twice to approve the test account. No real credentials.",
          }
        : {
            kind,
            content: `openkara-fixture:approve:${id}`,
            text: "Test QR: the second poll approves the fixture account. Do not scan with a real music app.",
          };
  }
  openkara.log.info("sign-in started", {
    challenge: id,
    presentation: presentation.kind,
  });
  return {
    challenge: id,
    expires_at: new Date(Date.now() + 60000).toISOString(),
    poll_after_ms: 1,
    presentation,
  };
}

async function pollAuth(params) {
  if (web) throw fault("unsupported_operation");
  if (!challenge || params.challenge !== challenge.id)
    throw fault("challenge_expired");
  challenge.polls += 1;
  await new Promise((resolve) => setTimeout(resolve, 1));
  if (challenge.polls < 2) return { state: "pending", poll_after_ms: 1 };
  return { state: "complete", session: signedIn(), credentials };
}

function completeAuth(params) {
  if (!web) throw fault("unsupported_operation");
  if (!challenge || params.challenge !== challenge.id)
    throw fault("challenge_expired");
  const cookie = manifest.authentication.cookie_names[0];
  if (!params.credentials || params.credentials[cookie] !== "verified-cookie") {
    openkara.log.warn("sign-in rejected", { challenge: challenge.id });
    throw fault("authentication_required");
  }
  return { session: signedIn(), credentials };
}

function cancelAuth() {
  challenge = null;
  return { state: "cancelled" };
}

function signOut() {
  session = { state: "signed_out", account: null };
  challenge = null;
  cursors.clear();
  openkara.storage.remove("account_id");
  openkara.log.info("signed out");
  return session;
}

function playlists() {
  requireSession("playlists");
  return { items: [playlist], next_cursor: null };
}

function playlistTracks(params) {
  requireSession("playlist.tracks");
  if (params.playlist_id !== playlist.id) throw fault("not_found");
  return { playlist, ...page(params, tracks) };
}

function tracksGet(params) {
  requireSession("tracks.get");
  const ids = Array.isArray(params.track_ids) ? params.track_ids : [];
  return {
    items: tracks.filter((track) => ids.includes(track.id)),
    missing_ids: ids.filter((id) => !tracks.some((track) => track.id === id)),
  };
}

function search(params) {
  return page(params, tracks);
}

async function resolveMedia(params) {
  requireSession("media.resolve");
  const track = tracks.find((item) => item.id === params.track_id);
  if (!track) throw fault("not_found");
  if (track.refusal) return { kind: "refusal", refusal: track.refusal };
  openkara.log.info("resolving media", {
    track_id: track.id,
    purpose: params.purpose ?? null,
  });
  const answer = await openkara.request({
    url: `${api}/sources/${manifest.source_id}/media/${track.id}`,
    responseType: "json",
  });
  if (answer.status !== 200)
    throw fault("network", "Fixture upstream connection failed");
  return { kind: "media", media: answer.body.media };
}

export default {
  describe,
  "session.restore": restore,
  "auth.start": startAuth,
  "auth.poll": pollAuth,
  "auth.complete": completeAuth,
  "auth.cancel": cancelAuth,
  "auth.sign_out": signOut,
  playlists,
  "playlist.tracks": playlistTracks,
  "tracks.get": tracksGet,
  search,
  liked_tracks: search,
  "media.resolve": resolveMedia,
};
