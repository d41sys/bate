import { test } from "node:test";
import assert from "node:assert/strict";
import { needsLines, unusable } from "./workplaces.ts";
import type { Workplace } from "./types.ts";

const base: Workplace = {
  name: "counter",
  version: "1",
  manifest: { name: "counter", version: "1", program: { command: "python3", args: [] }, needs: { folders: [], hosts: [], commands: [] } },
  skills: [],
  roles: [],
  changed: false,
  usedBy: [],
};

test("what a workplace needs is listed by kind, or said to be nothing", () => {
  assert.deepEqual(needsLines(base), ["Nothing beyond its own folder."]);
  const m = base.manifest!;
  const w = { ...base, manifest: { ...m, needs: { folders: ["~/code"], hosts: ["a.com", "b.com"], commands: [] } } };
  assert.deepEqual(needsLines(w), ["Folders: ~/code", "Network hosts: a.com, b.com"]);
});

test("a changed or broken workplace cannot be switched on", () => {
  assert.equal(unusable(base), undefined);
  assert.match(unusable({ ...base, changed: true })!, /changed/);
  assert.equal(unusable({ ...base, manifest: null, error: "no manifest" }), "no manifest");
});
