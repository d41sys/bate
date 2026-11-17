import { test } from "node:test";
import assert from "node:assert/strict";
import { needsProvider } from "./firstRun.ts";

test("the first run lasts until ChatGPT or a model server is set", () => {
  const none = { url: null, hasKey: false, fromEnv: false };
  assert.equal(needsProvider(false, none), true);
  assert.equal(needsProvider(true, none), false);
  assert.equal(needsProvider(false, { ...none, url: "http://127.0.0.1:1234/v1" }), false);
  assert.equal(needsProvider(false, { ...none, fromEnv: true }), false);
  // A key alone names no server.
  assert.equal(needsProvider(false, { ...none, hasKey: true }), true);
});
