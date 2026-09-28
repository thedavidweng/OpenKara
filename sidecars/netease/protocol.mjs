export const operations = [
  "user_account",
  "likelist",
  "user_playlist",
  "playlist_detail",
  "cloudsearch",
  "song_detail",
  "song_url",
];

export async function dispatch(api, request) {
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
      randomCNIP: true,
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
