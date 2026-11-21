import { test } from "node:test";
import assert from "node:assert/strict";
import { botFileLines } from "./botFile.ts";
import type { BotFilePreview } from "./types.ts";

const empty: BotFilePreview = { name: "Ada", instructionsLength: 0, skills: [], routines: [], notes: 0, connectors: [], sha256: "0" };

test("an empty bot file says what is not there", () => {
  assert.deepEqual(botFileLines(empty), [
    "Instructions: none",
    "Skills: none",
    "Routines (off until you turn them on): none",
    "Notes: 0",
    "Connectors (off until you turn them on): none",
  ]);
});

test("each thing the file holds is named, with when a routine runs and where a connector is", () => {
  const lines = botFileLines({
    ...empty,
    instructionsLength: 120,
    skills: [{ name: "pdf-forms", description: "d", files: 2 }],
    routines: [
      { name: "Brief", schedule: { frequency: "daily", interval: 1, time: "09:00" }, webhook: false, chat: false },
      { name: "Hook", schedule: null, webhook: true, chat: false },
    ],
    notes: 3,
    connectors: [{ name: "docs", address: "https://docs.example.com/mcp" }],
  });
  assert.equal(lines[0], "Instructions: 120 characters");
  assert.equal(lines[1], "Skills: pdf-forms");
  assert.equal(lines[2], "Routines (off until you turn them on): Brief (every day from 09:00), Hook (not on a schedule)");
  assert.equal(lines[3], "Notes: 3");
  assert.equal(lines[4], "Connectors (off until you turn them on): docs at https://docs.example.com/mcp");
});
