import { test } from "node:test";
import assert from "node:assert/strict";
import { agentChoice, OTHER, OWN, type Agent } from "./agents.ts";

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
