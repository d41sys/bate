import { test } from "node:test";
import assert from "node:assert/strict";
import { connectorOf, connectorState, connectorSummary, each, matches, placed } from "./addons.ts";
import type { Connector } from "./types.ts";

test("a skill three bots have is one row, with the bots that have it", () => {
  const pdf = { name: "pdf", description: "Forms" };
  const rows = placed([
    { bot: "a", items: [pdf, { name: "audit", description: "" }] },
    { bot: "b", items: [pdf] },
    { bot: "c", items: [pdf] },
  ]);
  assert.deepEqual(rows.map((r) => [r.name, r.users.map((u) => u.bot)]), [["audit", ["a"]], ["pdf", ["a", "b", "c"]]]);
});

test("one bot failing does not stop the others, and is named", async () => {
  const done: string[] = [];
  const failed = await each(["a", "b", "c"], async (id) => {
    if (id === "b") throw new Error("busy");
    done.push(id);
  });
  assert.deepEqual(done, ["a", "c"]);
  assert.deepEqual(failed, [{ id: "b", error: "busy" }]);
});

test("a connector's line says off, failed with why, or waiting for the sign-in", () => {
  const c: Connector = { name: "n", enabled: true, transport: { type: "http", url: "https://a.b/mcp" } };
  assert.equal(connectorState({ ...c, enabled: false, error: "x" }).text, "Off");
  assert.deepEqual(connectorState({ ...c, error: "no such program" }), { text: "Could not start: no such program", problem: true });
  assert.equal(connectorState({ ...c, oauth: true, signedIn: false }).problem, true);
  assert.equal(connectorState(c).problem, false);
  assert.equal(connectorState(c).text, "On, no problem reported");
  assert.deepEqual(connectorState({ ...c, tools: 3 }), { text: "Working, 3 tools", problem: false });
  assert.equal(connectorState({ ...c, tools: 1 }).text, "Working, 1 tool");
  assert.equal(connectorState({ ...c, tools: 3 }, true).text.startsWith("Working"), false);
});

test("a connector's row says on for how many bots, and how many need a look", () => {
  const c: Connector = { name: "n", enabled: true, transport: { type: "http", url: "https://a.b/mcp" }, oauth: true, signedIn: true };
  const copies = [c, c, c, c, { ...c, enabled: false }, { ...c, signedIn: false }];
  assert.deepEqual(connectorSummary(copies, 6), { text: "On for 5 of 6 bots", problems: 1 });
  assert.equal(connectorSummary(copies, 8).text, "On for 5 of 6 bots that have it");
  assert.equal(connectorSummary([c], 1).text, "On for 1 of 1 bot");
});

test("a program's arguments are one to a line, so none is split at a space", () => {
  const c = connectorOf({ name: " notes ", type: "stdio", url: "", command: "npx", args: "-y\n@acme/mcp server\n", oauth: true });
  assert.deepEqual(c.transport, { type: "stdio", command: "npx", args: ["-y", "@acme/mcp server"] });
  assert.equal(c.name, "notes");
  assert.equal(c.oauth, false);
});

test("a search needs every word, in any case, and no words match all", () => {
  assert.equal(matches("GHIDRA reverse", "ghidra", "Reverse engineering"), true);
  assert.equal(matches("ghidra web", "ghidra", "Reverse engineering"), false);
  assert.equal(matches("  ", "x"), true);
});
