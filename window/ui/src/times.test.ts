import assert from "node:assert/strict";
import { test } from "node:test";
import { dayTime, greeting, took } from "./times.ts";

// Saturday 21 November 2026, 10:00, local time: the expected clock is the system's own, whatever its locale.
const now = new Date(2026, 10, 21, 10, 0).getTime();
const at = (day: number, h: number, m: number, month = 10, year = 2026) => new Date(year, month, day, h, m).getTime();
const clock = (ms: number) => new Date(ms).toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });

test("today, tomorrow and yesterday say so, with the time", () => {
  assert.equal(dayTime(at(21, 23, 59), now), `Today ${clock(at(21, 23, 59))}`);
  assert.equal(dayTime(at(22, 8, 0), now), `Tomorrow ${clock(at(22, 8, 0))}`);
  assert.equal(dayTime(at(20, 11, 41), now), `Yesterday ${clock(at(20, 11, 41))}`);
});

test("a day is a calendar day, not 24 hours", () => {
  const late = new Date(2026, 10, 21, 23, 30).getTime();
  assert.match(dayTime(at(22, 0, 10), late), /^Tomorrow /);
  assert.match(dayTime(at(20, 23, 59), new Date(2026, 10, 21, 0, 5).getTime()), /^Yesterday /);
});

test("within six days either way it is the weekday and the time", () => {
  const out = dayTime(at(25, 17, 30), now);
  assert.ok(out.endsWith(` ${clock(at(25, 17, 30))}`));
  assert.ok(!/^(Today|Tomorrow|Yesterday)/.test(out));
  assert.ok(dayTime(at(15, 9, 0), now).endsWith(` ${clock(at(15, 9, 0))}`));
});

test("older or further off is the date alone, with the year when it is not this one", () => {
  const old = dayTime(at(5, 9, 0), now);
  assert.ok(!old.includes(clock(at(5, 9, 0))));
  assert.ok(!old.includes("2026"));
  assert.ok(dayTime(at(5, 9, 0, 10, 2025), now).includes("2025"));
  assert.ok(!dayTime(at(12, 9, 0, 11), now).includes(clock(at(12, 9, 0))));
});

test("how long something took is in the largest two units", () => {
  assert.equal(took(0), "0 s");
  assert.equal(took(12_400), "12 s");
  assert.equal(took(59_600), "1 min 0 s");
  assert.equal(took(125_000), "2 min 5 s");
  assert.equal(took(3_840_000), "1 h 4 min");
  assert.equal(took(-5), "0 s");
});

test("the greeting follows the hour, and the small hours are late, not morning", () => {
  assert.deepEqual([0, 4, 5, 11, 12, 17, 18, 23].map(greeting), ["It's late.", "It's late.", "Good morning.", "Good morning.", "Good afternoon.", "Good afternoon.", "Good evening.", "Good evening."]);
});
