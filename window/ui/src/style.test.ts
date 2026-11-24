import { test } from "node:test";
import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";

// docs/design-system.md, rule 3: an inline style is for a value the code computes. Each one, with why.
const STYLES: Record<string, string[]> = {
  "Avatar.tsx": ["a bot's colour, handed to CSS as --tint"],
  "Chat.tsx": ["the height of the virtual list", "where the virtual list puts a row"],
  "Pane.tsx": ["the width being dragged"],
  "Surfaces.tsx": ["how deep a node is, handed to CSS as --depth"],
};
// A colour is a token of style.css. The one literal: what a colour field shows before a colour is chosen.
const COLOURS: Record<string, number> = { "Settings.tsx": 1 };

test("an inline style or a colour literal in the window's code is one that is listed", () => {
  const dir = new URL("./", import.meta.url);
  for (const file of readdirSync(dir).filter((f) => f.endsWith(".tsx"))) {
    const source = readFileSync(new URL(file, dir), "utf8");
    const count = (pattern: RegExp) => (source.match(pattern) ?? []).length;
    assert.equal(count(/\bstyle=/g), STYLES[file]?.length ?? 0, `${file}: looks go in style.css; an inline style for a value the code computes is listed in STYLES with why`);
    assert.equal(count(/["'`]#([0-9a-f]{3,4}|[0-9a-f]{6}|[0-9a-f]{8})["'`]|\b(rgb|hsl)a?\(/gi), COLOURS[file] ?? 0, `${file}: a colour is a token of style.css, not a literal`);
  }
});
