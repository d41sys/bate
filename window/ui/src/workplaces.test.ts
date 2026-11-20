import { test } from "node:test";
import assert from "node:assert/strict";
import { downloadText, needsLines, progressText, sizeText, unusable } from "./workplaces.ts";
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

test("the person is told the size and the hosts before the downloads start", () => {
  const m = base.manifest!;
  const dl = { url: "https://a.com/x", size: 1, sha256: "", dest: "x" };
  assert.equal(downloadText({ manifest: m, downloadBytes: 0, hosts: [] }), undefined);
  const p = { manifest: { ...m, downloads: [dl, dl] }, downloadBytes: 3 * 1024 ** 3 / 2, hosts: ["a.com", "b.com:8080"] };
  assert.equal(downloadText(p), "Installing counter downloads 2 files, 1.5 GiB in all, from a.com, b.com:8080.");
  assert.equal(sizeText(5), "5 B");
  assert.equal(sizeText(300 * 1024 ** 2), "300.0 MiB");
  assert.equal(progressText(1024, 4096), "Downloading 1.0 KiB of 4.0 KiB (25%)");
});
