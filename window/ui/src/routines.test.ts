import assert from "node:assert/strict";
import { test } from "node:test";
import { afterText, checkText, heldText, skipWhy, watchArguments, watchText, watchTools } from "./routines.ts";
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

test("the tools to watch are the connector's own, each marked when the bot would not be let call it alone", () => {
  const issues = { name: "issues", enabled: true, transport: { type: "stdio" as const, command: "serve" }, toolNames: ["issues__list", "issues__close", "issues__get"], blocked: ["issues__close"] };
  const allow = (level: "always" | "read_only" | "ask" | "never", grants: { kind: "connector"; target: string; atMs: number }[] = []) => ({ levels: { connector: level } as never, grants });
  assert.deepEqual(watchTools(issues, "", allow("always"))?.map((t) => t.label), ["close (blocked)", "get", "list"]);
  assert.deepEqual(watchTools(issues, "", allow("ask", [{ kind: "connector", target: "issues__get", atMs: 1 }]))?.map((t) => t.label), ["close (blocked)", "get", "list (would ask)"]);
  assert.deepEqual(watchTools(issues, "", allow("never"))?.map((t) => t.label), ["close (blocked)", "get (not allowed)", "list (not allowed)"]);
  // At "read-only tools without asking" a tool runs alone when it is marked and confirmed; one still to confirm, or not marked, would ask.
  const marked = { ...issues, toolNames: [...issues.toolNames, "issues__peek"], readOnly: ["issues__get", "issues__peek"], toConfirm: ["issues__peek"] };
  assert.deepEqual(watchTools(marked, "", allow("read_only"))?.map((t) => t.label), ["close (blocked)", "get", "list (would ask)", "peek (would ask)"]);
  assert.equal(watchTools(issues, "")?.[2].label, "list");
  // A tool already set that the engine did not list stays in the list; a connector never loaded has no list.
  assert.equal(watchTools(issues, "gone", allow("always"))?.some((t) => t.own === "gone"), true);
  assert.equal(watchTools({ ...issues, toolNames: undefined }, "", allow("always")), undefined);
  assert.equal(watchTools(undefined, ""), undefined);
});
