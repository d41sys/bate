import { test } from "node:test";
import assert from "node:assert/strict";
import { adapterFacts, agentChoice, OTHER, OWN, type AdapterPreview, type Agent } from "./agents.ts";

const opencode: Agent = { label: "opencode", command: ["opencode", "acp"], path: "/opt/bin/opencode" };
const codex: Agent = { label: "Codex", command: ["codex-acp"], path: "/opt/bin/codex-acp" };

test("no saved command is Bate's own engine", () => {
  assert.equal(agentChoice(null, [opencode]), OWN);
  assert.equal(agentChoice(undefined, []), OWN);
  assert.equal(agentChoice([], [opencode]), OWN);
});

test("the exact command of an installed agent selects it", () => {
  assert.equal(agentChoice(["opencode", "acp"], [codex, opencode]), "opencode");
});

test("a known agent that is not installed now is Other", () => {
  assert.equal(agentChoice(["opencode", "acp"], [codex]), OTHER);
  assert.equal(agentChoice(["opencode", "acp"], []), OTHER);
});

test("any other command is Other, also one that starts like an installed agent's", () => {
  assert.equal(agentChoice(["my-agent", "--acp"], [opencode]), OTHER);
  assert.equal(agentChoice(["opencode"], [opencode]), OTHER);
  assert.equal(agentChoice(["opencode", "acp", "--debug"], [opencode]), OTHER);
});

const preview: AdapterPreview = {
  name: "codex-acp", description: "d", version: "2.9.0", home: "https://example.com", installedVersion: null, update: false,
  packages: 25, bytes: 160 * 1024 * 1024, hosts: ["registry.npmjs.org"], node: 20, runtime: { path: "/opt/node", version: "v22.1.0" }, missing: null,
};

test("the preview says what is fetched, from where, and the node it runs on", () => {
  const [fetched, checked, runtime] = adapterFacts(preview);
  assert.equal(fetched, "Version 2.9.0: 25 packages, 160.0 MiB to download from registry.npmjs.org.");
  assert.match(checked, /No install script is run/);
  assert.equal(runtime, "It runs on the Node.js you have: v22.1.0, /opt/node.");
});

test("without a usable node the preview says which one is needed", () => {
  const [, , runtime] = adapterFacts({ ...preview, runtime: null, missing: "Node.js is not installed" });
  assert.equal(runtime, "It needs Node.js 20 or newer, which Bate does not install.");
});
