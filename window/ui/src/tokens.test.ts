import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

// docs/design-system.md, Accessibility: text has 4.5:1 on what it lies on, a field's edge and a control's fill 3:1, in light and in dark.
// The pictures show one mode at a time and no number; this reads the tokens and works the numbers out.
const css = readFileSync(new URL("./tokens.css", import.meta.url), "utf8");
const [lightBlock, rest] = css.split("@media (prefers-color-scheme: dark)");
const read = (block: string) => Object.fromEntries([...block.matchAll(/(--[a-z0-9-]+):\s*([^;]+);/g)].map((m) => [m[1], m[2].trim()]));
const light = read(lightBlock);
const dark = { ...light, ...read(rest.split("@media (prefers-reduced-motion")[0]) };

type Colour = { rgb: number[]; alpha: number };
/** An `oklch()` token as linear sRGB, which is what luminance is worked out from. */
function colour(tokens: Record<string, string>, name: string): Colour {
  let value = tokens[name];
  while (value?.startsWith("var(")) value = tokens[value.slice(4, -1)];
  const m = value?.match(/^oklch\(([\d.]+)%\s+([\d.]+)\s+([\d.]+)(?:\s*\/\s*([\d.]+))?\)$/);
  assert.ok(m, `${name} is an oklch() colour: ${value}`);
  const L = Number(m[1]) / 100, C = Number(m[2]), h = (Number(m[3]) * Math.PI) / 180;
  const a = C * Math.cos(h), b = C * Math.sin(h);
  const l = (L + 0.3963377774 * a + 0.2158037573 * b) ** 3, s = (L - 0.1055613458 * a - 0.0638541728 * b) ** 3, t = (L - 0.0894841775 * a - 1.291485548 * b) ** 3;
  const rgb = [4.0767416621 * l - 3.3077115913 * s + 0.2309699292 * t, -1.2684380046 * l + 2.6097574011 * s - 0.3413193965 * t, -0.0041960863 * l - 0.7034186147 * s + 1.707614701 * t];
  return { rgb: rgb.map((x) => Math.min(1, Math.max(0, x))), alpha: m[4] === undefined ? 1 : Number(m[4]) };
}
const luminance = (c: number[]) => 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
const ratio = (a: number[], b: number[]) => (Math.max(luminance(a), luminance(b)) + 0.05) / (Math.min(luminance(a), luminance(b)) + 0.05);

// What lies on what, and the least it may have.
const PAIRS: [string, string, number][] = [
  ["--fg", "--bg", 4.5], ["--fg", "--bg-soft", 4.5], ["--fg", "--bg-soft-3", 4.5], ["--fg", "--bg-thumb", 4.5], ["--fg", "--bg-current", 4.5],
  ["--fg-muted", "--bg", 4.5], ["--fg-muted", "--bg-side", 4.5], ["--fg-muted", "--bg-soft", 4.5], ["--fg-muted", "--bg-soft-2", 4.5], ["--fg-muted", "--bg-raised", 4.5], ["--fg-muted", "--bg-current", 4.5],
  ["--on-ink", "--ink", 4.5], ["--on-danger", "--danger", 4.5], ["--on-bubble-user", "--bubble-user", 4.5], ["--warn", "--warn-soft", 4.5],
  ["--accent", "--bg", 4.5], ["--accent", "--bg-soft", 4.5], ["--danger", "--bg", 4.5], ["--danger", "--bg-soft", 4.5], ["--danger", "--bg-raised", 4.5],
  ["--border-field", "--bg", 3], ["--border-field", "--bg-soft", 3], ["--border-field", "--bg-raised", 3], ["--border-field", "--bg-field", 3],
  ["--ink", "--bg", 3], ["--ink", "--bg-side", 3], ["--accent", "--bg-side", 3],
];

for (const [mode, tokens] of [["light", light], ["dark", dark]] as const) {
  test(`in ${mode}, text, a field's edge and the main fill can be told from what they lie on`, () => {
    for (const [over, under, least] of PAIRS) {
      const ground = colour(tokens, under).rgb;
      const top = colour(tokens, over);
      const seen = top.rgb.map((x, i) => x * top.alpha + ground[i] * (1 - top.alpha));
      const got = ratio(seen, ground);
      assert.ok(got >= least, `${over} on ${under} is ${got.toFixed(2)}:1, under ${least}:1`);
    }
  });
}
