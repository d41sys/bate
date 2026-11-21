import type { Question } from "./chat.ts";
import { hasHidden } from "./reveal.ts";

/** What the system's notification for a waiting question says, and whether it has the buttons "Allow once" and "Deny". */
export type Notice = { title: string; body: string; actions: boolean };

/**
 * The kinds that are asked about once and may be granted. Not here: `apple_change` (a send or change in an
 * Apple app is asked about every time, in the window), and a question with no kind (the person is
 * asked each time). A card of those opens the window.
 */
const BUTTONS_FOR = ["read", "write", "run", "connector", "apple_read"];

/** How much of the target a notification shows, in characters. */
const TARGET = 160;

/**
 * Buttons only for a question the notification shows whole: one of the kinds above, with both plain
 * choices, a target that is one line and is not cut, and no hidden or direction-changing character in
 * anything it shows (the notification cannot mark them, so it shows none of it). A button must never
 * allow text the person was not shown. "Always" is never offered here: it is a card's choice.
 */
export function noticeFor(bot: string, q: Question): Notice {
  if (hasHidden(bot, q.title, q.target, q.folder, ...(q.askedBy ?? []))) {
    return { title: "A bot needs you", body: "Open Bate to read what it asks: some characters in it cannot be seen.", actions: false };
  }
  const flat = (q.target ?? "").replace(/\s+/g, " ").trim();
  const letters = Array.from(flat);
  const cut = letters.length > TARGET;
  const target = cut ? `${letters.slice(0, TARGET).join("")}… (cut: open Bate to read all of it)` : flat;
  const whole = !cut && !/[\r\n]/.test(q.target ?? "") && Array.from(q.title).length <= TARGET;
  return {
    title: "Needs you",
    body: [`${bot}: ${q.title}`, target].filter(Boolean).join("\n"),
    actions: whole && !!q.kind && BUTTONS_FOR.includes(q.kind) && q.options.includes("Approve") && q.options.includes("Deny"),
  };
}
