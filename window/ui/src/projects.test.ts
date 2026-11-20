import { test } from "node:test";
import assert from "node:assert/strict";
import { findBots, folderName, takenBy } from "./projects.ts";
import type { Bot, Project } from "./types.ts";

const bot = (name: string, project?: string) => ({ id: name, name, project: project ? { id: project, name: `Site ${project}` } : undefined }) as Bot;

test("a folder's name is the project's name, with or without a closing slash", () => {
  assert.equal(folderName("/Users/me/code/my-site"), "my-site");
  assert.equal(folderName("/Users/me/code/my-site/"), "my-site");
  assert.equal(folderName(""), "");
});

test("the list of bots narrows to the words typed, whatever their case", () => {
  const all = [bot("Analyst"), bot("Daisy"), bot("Data cleaner")];
  assert.deepEqual(findBots(all, " DA ").map((b) => b.name), ["Daisy", "Data cleaner"]);
  assert.equal(findBots(all, "").length, 3);
});

test("a bot of another project is named with that project and cannot be ticked; one of this project can", () => {
  const here = { id: "p1", name: "Site p1", folder: "", bots: [] } as Project;
  assert.equal(takenBy(bot("a", "p2"), here), "Site p2");
  assert.equal(takenBy(bot("b", "p1"), here), undefined);
  assert.equal(takenBy(bot("c"), here), undefined);
  assert.equal(takenBy(bot("d", "p2")), "Site p2");
});
