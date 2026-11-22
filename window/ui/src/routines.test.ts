import assert from "node:assert/strict";
import { test } from "node:test";
import { afterText, heldText, skipWhy } from "./routines.ts";
import { dayTime } from "./times.ts";

test("a skipped run says why, and names the connector or the precheck's line", () => {
  assert.equal(skipWhy({}), undefined);
  assert.equal(skipWhy({ reason: "paused" }), "The bot was paused.");
  assert.equal(skipWhy({ reason: "signed_out", detail: "notes" }), "The connector notes needs a sign-in.");
  assert.equal(skipWhy({ reason: "precheck", detail: "nothing new" }), "The precheck stopped it: nothing new");
  assert.equal(skipWhy({ reason: "limit", detail: "the bot used its limit of 50 tokens for today; it counts again from midnight" }), "The token limit stopped it: the bot used its limit of 50 tokens for today; it counts again from midnight.");
  assert.equal(skipWhy({ reason: "busy" }), "The bot was busy.");
  assert.equal(skipWhy({ reason: "rate" }), "Held back: it already started 20 times in the last hour.");
  assert.equal(skipWhy({ reason: "something new" }), "something new");
});

test("the badge of a held routine has the count and the next run, if there is one", () => {
  const now = new Date(2026, 10, 21, 10, 0).getTime();
  const next = new Date(2026, 10, 22, 9, 0).getTime();
  assert.equal(heldText({ failures: 3, nextRunMs: next }, now), `3 failed in a row · next run ${dayTime(next, now)}`);
  assert.equal(heldText({ failures: 7 }, now), "7 failed in a row");
});

test("a routine that follows another bot says which bot and how its turn must end", () => {
  assert.equal(afterText({ bot: "b1", outcome: "done" }, "Ada"), "After Ada finishes a turn");
  assert.equal(afterText({ bot: "b1", outcome: "failed" }, "Ada"), "After Ada fails a turn");
  assert.equal(afterText({ bot: "b1", outcome: "any" }), "After b1 ends a turn");
});
