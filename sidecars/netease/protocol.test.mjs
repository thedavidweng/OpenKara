import { test } from "node:test";
import assert from "node:assert/strict";
import { dispatch } from "./protocol.mjs";

test("returns provider bodies and preserves rejection codes without credentials", async () => {
  const api = {
    user_account: async (query) => {
      assert.equal(query.cookie, "MUSIC_U=session; __csrf=csrf");
      assert.equal(query.randomCNIP, true);
      assert.equal(query.unblock, "false");
      return { body: { code: 200, account: { id: 42 } }, cookie: ["secret"] };
    },
    cloudsearch: async () => {
      throw { body: { code: 8860, message: "rejected", cookie: "secret" } };
    },
  };
  assert.deepEqual(
    await dispatch(api, {
      operation: "user_account",
      params: {},
      cookie: "MUSIC_U=session; __csrf=csrf",
    }),
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
