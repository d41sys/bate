import type { Question } from "./chat.ts";

/** One conversation that is not idle (`list_attention`). */
export type AttentionItem = {
  bot: string;
  conversation: string;
  /** The name or title; empty before the bot's first reply. */
  title: string;
  /** When it last changed, in Unix milliseconds. */
  atMs: number;
  busy: boolean;
  questions: Question[];
  /** The last line of the bot's latest reply. */
  line: string;
  /** How the turn the person has not seen ended. */
  ended: "done" | "limit" | "error" | null;
  error: string | null;
};

export type Section = "needs" | "working" | "done" | "failed";

/** The groups, in the order the screen shows them. */
export const SECTIONS: { id: Section; title: string }[] = [
  { id: "needs", title: "Needs you" },
  { id: "working", title: "Working" },
  { id: "done", title: "Done since you last looked" },
  { id: "failed", title: "Failed" },
];

/** The one group a conversation is in, the most urgent first: a question, a turn that runs, how the last turn ended. None when it is idle. */
export const sectionOf = (i: AttentionItem): Section | undefined => (i.questions.length > 0 ? "needs" : i.busy ? "working" : i.ended === "done" ? "done" : i.ended ? "failed" : undefined);

/** How many conversations need the person: the Dock's count, and the size of "Needs you" on the Activity screen. */
export const needsCount = (items: AttentionItem[]) => items.filter((i) => sectionOf(i) === "needs").length;

/** Whether `frame` may change the list (`list_attention`): ask again after it. */
export function changesAttention(frame: { type: string; entry?: unknown }): boolean {
  const entry = frame.entry as { type?: string } | undefined;
  return ["bots_changed", "conversations_changed", "question_closed"].includes(frame.type) || (frame.type === "transcript" && (entry?.type === "turn_end" || entry?.type === "question"));
}

/** The groups that have a card, in order, each with its newest first. */
export function grouped(items: AttentionItem[]): { id: Section; title: string; items: AttentionItem[] }[] {
  const newest = [...items].sort((a, b) => b.atMs - a.atMs);
  return SECTIONS.map((s) => ({ ...s, items: newest.filter((i) => sectionOf(i) === s.id) })).filter((g) => g.items.length > 0);
}

/** What a card says under the name: the question asks for itself, the rest give the last line or why it failed. */
export function said(i: AttentionItem): string {
  const section = sectionOf(i);
  if (section === "failed") return i.error || (i.ended === "limit" ? "Stopped at its step limit." : "Stopped with an error.");
  return i.line || (section === "working" ? "Working…" : "Finished.");
}

/** How long ago, short: "Just now", "5 min ago", "3 h ago", "2 days ago". */
export function ago(ms: number, now = Date.now()): string {
  const minutes = Math.floor((now - ms) / 60_000);
  if (minutes < 1) return "Just now";
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours} h ago`;
  const days = Math.floor(hours / 24);
  return days === 1 ? "1 day ago" : `${days} days ago`;
}

/** `items` with the question the person just answered taken out; what the service says next replaces it. */
export const answered = (items: AttentionItem[], conversation: string, question: string): AttentionItem[] =>
  items.map((i) => (i.conversation === conversation ? { ...i, questions: i.questions.filter((q) => q.question !== question) } : i));
