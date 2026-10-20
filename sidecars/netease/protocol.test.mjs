import { test } from "node:test";
import assert from "node:assert/strict";
import { dispatch, handleLine, serve } from "./protocol.mjs";

test("returns provider bodies and preserves rejection codes without credentials", async () => {
  const api = {
    user_account: async (query) => {
      assert.equal(query.cookie, "MUSIC_U=session; __csrf=csrf");
      assert.equal(query.realIP, "211.161.244.70");
      assert.equal(query.unblock, "false");
      return { body: { code: 200, account: { id: 42 } }, cookie: ["secret"] };
    },
    cloudsearch: async () => {
      throw { body: { code: 8860, message: "rejected", cookie: "secret" } };
    },
  };
  assert.deepEqual(
    await dispatch(
      api,
      {
        operation: "user_account",
        params: {},
        cookie: "MUSIC_U=session; __csrf=csrf",
      },
      "211.161.244.70",
    ),
    { body: { code: 200, account: { id: 42 } } },
  );
  assert.deepEqual(
    await dispatch(api, { operation: "cloudsearch", params: {}, cookie: "" }),
    { body: { code: 8860 } },
  );
  assert.deepEqual(
    await dispatch(api, {
      operation: "login_cellphone",
      params: {},
      cookie: "",
    }),
    { error: "Unsupported NetEase operation" },
  );
});

test("echoes request ids and runs at most the limit concurrently", async () => {
  let active = 0;
  let peak = 0;
  const api = {
    song_detail: async (query) => {
      active += 1;
      peak = Math.max(peak, active);
      await new Promise((resolve) => setTimeout(resolve, query.delay));
      active -= 1;
      return { body: { code: 200, songs: [query.delay] } };
    },
  };
  const lines = [30, 20, 10, 5, 1].map((delay, index) =>
    JSON.stringify({
      id: index + 1,
      operation: "song_detail",
      params: { delay },
      cookie: "",
    }),
  );
  lines.push("not json");
  const responses = [];
  await serve(
    lines,
    (line) => handleLine(api, line, "211.161.244.70"),
    (response) => responses.push(response),
    2,
  );
  assert.equal(peak, 2);
  assert.equal(responses.length, 6);
  assert.deepEqual(
    responses
      .filter((response) => response.body)
      .map((response) => response.id)
      .sort(),
    [1, 2, 3, 4, 5],
  );
  assert.deepEqual(
    responses.find((response) => response.error),
    { id: undefined, error: "Invalid NetEase request" },
  );
  assert.equal(
    responses.find((response) => response.id === 1).body.songs[0],
    30,
  );
});
