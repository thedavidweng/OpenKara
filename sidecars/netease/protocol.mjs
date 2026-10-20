export const operations = [
  "user_account",
  "likelist",
  "user_playlist",
  "playlist_detail",
  "cloudsearch",
  "song_detail",
  "song_url",
];

export async function dispatch(api, request, chinaClientAddress) {
  if (!operations.includes(request?.operation))
    return { error: "Unsupported NetEase operation" };
  if (
    typeof request.cookie !== "string" ||
    !request.params ||
    typeof request.params !== "object" ||
    Array.isArray(request.params)
  )
    return { error: "Invalid NetEase request" };
  try {
    const response = await api[request.operation]({
      ...request.params,
      cookie: request.cookie,
      timeout: 30000,
      unblock: "false",
      // The API module honours randomCNIP only in its HTTP server; direct calls
      // send X-Real-IP from realIP alone (ADR 0033).
      realIP: chinaClientAddress,
    });
    if (response.body?.code !== 200)
      return { body: { code: response.body?.code ?? 502 } };
    const body = { ...response.body };
    delete body.cookie;
    return { body };
  } catch (error) {
    if (Number.isInteger(error?.body?.code))
      return { body: { code: error.body.code } };
    return { error: "NetEase request failed" };
  }
}

// Maximum requests the worker runs at once (ADR 0035 keeps one process).
export const maxConcurrentRequests = 4;

/** Dispatches one protocol line and echoes its request id in the response. */
export async function handleLine(api, line, chinaClientAddress) {
  let id;
  try {
    const request = JSON.parse(line);
    id = request?.id;
    return { id, ...(await dispatch(api, request, chinaClientAddress)) };
  } catch {
    return { id, error: "Invalid NetEase request" };
  }
}

/**
 * Handles lines concurrently (at most `limit` in flight) and calls `write`
 * with each response as soon as it settles. Resolves when input ends and all
 * in-flight requests are done.
 */
export async function serve(
  lines,
  handle,
  write,
  limit = maxConcurrentRequests,
) {
  const inFlight = new Set();
  for await (const line of lines) {
    if (!line.trim()) continue;
    const task = handle(line)
      .then(write)
      .finally(() => inFlight.delete(task));
    inFlight.add(task);
    if (inFlight.size >= limit) await Promise.race(inFlight);
  }
  await Promise.all(inFlight);
}
