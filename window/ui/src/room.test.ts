import { test } from "node:test";
import assert from "node:assert/strict";
import { leadOf, mentionAt, withoutMention } from "./room.ts";
import type { Bot, Project } from "./types.ts";

const bot = (id: string, name: string) => ({ id, name }) as Bot;
const [ann, bo] = [bot("a", "Ann"), bot("b", "Bo Peep")];

test("typing @ lists the bots whose names start with what follows, names with spaces too", () => {
  assert.deepEqual(mentionAt("@", 1, [ann, bo])?.matches, [ann, bo]);
  assert.deepEqual(mentionAt("hi @bo p", 8, [ann, bo])?.matches, [bo]);
  assert.equal(mentionAt("hi @bo p", 8, [ann, bo])?.start, 3);
  // An address in a word, or a name nobody has, opens nothing.
  assert.equal(mentionAt("mail me@bo", 10, [ann, bo]), undefined);
  assert.equal(mentionAt("@zed", 4, [ann, bo]), undefined);
});

test("a picked bot leaves the words around it as they were", () => {
  assert.equal(withoutMention("@An look", 0, 3), "look");
  assert.equal(withoutMention("see @B there", 4, 6), "see there");
});

test("a message with no @ goes to the lead, else the first bot of the project", () => {
  const project = { name: "P", folder: "", bots: ["a", "b"], lead: "b" } as Project;
  assert.equal(leadOf(project, [ann, bo]), bo);
  assert.equal(leadOf({ ...project, lead: "gone" }, [ann, bo]), ann);
  assert.equal(leadOf({ ...project, bots: [] }, []), undefined);
});
