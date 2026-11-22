// Quick Ask's page logic (PROTOCOL.md, "Quick Ask"): the shortcut as the person types it, what is sent,
// the order of the bots, and the answer so far.

import type { Item } from "./chat.ts";
import type { Bot } from "./types.ts";

/** What the service keeps for Quick Ask (`get_quick_ask`). */
export type QuickSettings = { shortcut: string | null; selection: boolean; screenshot: boolean };
/** What the panel opens with (`quick_context`): what each switch that is on found, or why it could not. */
export type QuickContext = {
  quote: string | null;
  quoteError: string | null;
  windows: { id: number; app: string; title: string }[] | null;
  windowsError: string | null;
};

type Pressed = { code: string; ctrlKey: boolean; altKey: boolean; shiftKey: boolean; metaKey: boolean };

/**
 * The shortcut a key press makes, in the service's words (`Ctrl+Alt+Space`): by the physical key, with
 * Ctrl, Alt or Cmd held. A press that cannot be a shortcut (no such modifier, or a key the Mac does not
 * take for one) is `undefined`.
 */
export function shortcutOf(e: Pressed): string | undefined {
  const key = /^Key([A-Z])$/.exec(e.code)?.[1] ?? /^Digit(\d)$/.exec(e.code)?.[1] ?? (/^(Space|F([1-9]|1[0-2]))$/.test(e.code) ? e.code : undefined);
  if (!key || !(e.ctrlKey || e.altKey || e.metaKey)) return undefined;
  return [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Cmd", key].filter(Boolean).join("+");
}

const SIGNS: Record<string, string> = { Ctrl: "⌃", Alt: "⌥", Shift: "⇧", Cmd: "⌘" };
/** `Shift+Cmd+Space` as the Mac writes it. */
export const display = (shortcut: string) => shortcut.split("+").map((part) => SIGNS[part] ?? part).join("");

/**
 * What is sent: the question, then the quoted text as a block quote, then the files as the chat lists
 * them (`Attached:` and a line for each path in the bot's folder). Empty when there is no question.
 */
export function message(text: string, quote: string | undefined, paths: string[]): string {
  if (!text.trim()) return "";
  const quoted = quote?.trim() ? `Selected text:\n${quote.trim().split("\n").map((line) => `> ${line}`).join("\n")}` : "";
  const attached = paths.length ? `Attached:\n${paths.map((path) => `- ${path}`).join("\n")}` : "";
  return [text.trim(), quoted, attached].filter(Boolean).join("\n\n");
}

/** The bots, those last asked through Quick Ask first (newest first), then the rest in their own order. */
export const usedFirst = (bots: Bot[], used: string[]) => [
  ...used.flatMap((id) => bots.filter((b) => b.id === id)),
  ...bots.filter((b) => !used.includes(b.id)),
];

/** `id` is the newest of the bots used; the list keeps at most 50. */
export const used = (ids: string[], id: string) => [id, ...ids.filter((i) => i !== id)].slice(0, 50);

const STORE = "bate-quick-bots";

export function loadUsed(): string[] {
  try {
    const ids: unknown = JSON.parse(localStorage.getItem(STORE) ?? "[]");
    return Array.isArray(ids) ? ids.filter((i): i is string => typeof i === "string") : [];
  } catch {
    return [];
  }
}

export function saveUsed(ids: string[]) {
  try {
    localStorage.setItem(STORE, JSON.stringify(ids));
  } catch {
    // A full or blocked store only means the order is not kept.
  }
}

/** What the bot said to the question just asked: its replies since the last message of the person, and how the turn ended, if it has. */
export function answerOf(items: Item[]): { text: string; ended?: { reason: string; error?: string } } {
  const after = items.slice(items.map((i) => i.kind).lastIndexOf("user") + 1);
  const end = after.find((i) => i.kind === "ended");
  return {
    text: after.flatMap((i) => (i.kind === "reply" ? [i.text] : [])).join("\n\n"),
    ended: end?.kind === "ended" ? { reason: end.reason, error: end.error } : undefined,
  };
}
