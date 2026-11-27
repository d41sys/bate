import { test } from "node:test";
import assert from "node:assert/strict";
import { FIRST_BOT, needsProvider, STARTERS } from "./firstRun.ts";
import { BOT_ICONS } from "./icons.ts";

test("the first run lasts until ChatGPT or a model server is set", () => {
  const none = { url: null, hasKey: false, fromEnv: false };
  assert.equal(needsProvider(false, none), true);
  assert.equal(needsProvider(true, none), false);
  assert.equal(needsProvider(false, { ...none, url: "http://127.0.0.1:1234/v1" }), false);
  // What a service with nothing set says: it names the environment as the place to look, and finds no address.
  assert.equal(needsProvider(false, { ...none, fromEnv: true }), true);
  assert.equal(needsProvider(false, { url: "http://127.0.0.1:1234/v1", hasKey: false, fromEnv: true }), false);
  // A key alone names no server.
  assert.equal(needsProvider(false, { ...none, hasKey: true }), true);
});

test("the first bot and the starters fit what the service accepts (PROTOCOL.md: create_bot, set_bot)", () => {
  for (const s of [FIRST_BOT, ...STARTERS]) {
    assert.ok(s.name.length >= 1 && s.name.length <= 80, s.name);
    assert.ok(s.purpose.length >= 1 && s.purpose.length <= 8000, s.name);
    assert.ok(BOT_ICONS.has(s.avatar), `${s.name}: "${s.avatar}" is an icon of the set`);
    assert.ok([...BOT_ICONS.keys()].every((key) => key.length <= 8), "an icon's name fits an avatar");
    // The purpose is the first line of the instructions.
    assert.ok(!s.purpose.includes("\n"), s.name);
  }
});
