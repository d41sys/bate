import assert from "node:assert/strict";
import { test } from "node:test";
import { afterText, checkText, heldText, skipWhy, watchArguments, watchText } from "./routines.ts";
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

test("a watch and its last check are said in words, an error in place of the verdict", () => {
  const now = new Date(2026, 10, 21, 10, 0).getTime();
  const watch = { connector: "issues", tool: "list", arguments: {}, every: 5 };
  assert.equal(watchText(watch), "Watches issues: list, every 5 minutes");
  const check = { atMs: now, changed: false, hash: "h", last: "x" };
  assert.equal(checkText(check, now), `Last check ${dayTime(now, now)}: no change`);
  assert.equal(checkText({ ...check, changed: true }, now), `Last check ${dayTime(now, now)}: the answer changed`);
  assert.equal(checkText({ ...check, error: "server down" }, now), `Last check ${dayTime(now, now)}: server down`);
});

test("the arguments of a watch are a JSON object, or empty for none", () => {
  assert.deepEqual(watchArguments(""), {});
  assert.deepEqual(watchArguments(' {"state": "open"} '), { state: "open" });
  assert.throws(() => watchArguments("{state"), /must be JSON/);
  assert.throws(() => watchArguments("[1]"), /must be a JSON object/);
  assert.throws(() => watchArguments("null"), /must be a JSON object/);
});
