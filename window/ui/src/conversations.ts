import type { Conversation } from "./types.ts";

/** What the person sees: the name they gave, else the title the first reply made, else that it has none yet. */
export const label = (c: Pick<Conversation, "name" | "title">) => c.name || c.title || "Untitled";

/** When, short: "Today" and the time for today, else the day, with the year when it is not this one. */
export function when(ms: number, now = Date.now()): string {
  const at = new Date(ms);
  const today = new Date(now);
  if (at.toDateString() === today.toDateString()) return `Today, ${at.toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" })}`;
  const year = at.getFullYear() !== today.getFullYear();
  return at.toLocaleDateString(undefined, { day: "numeric", month: "short", ...(year && { year: "numeric" as const }) });
}

/** The list the switcher shows: archived ones only on request, and never the one that is open. */
export const listed = (all: Conversation[], open: string, archived: boolean) => all.filter((c) => archived || !c.archived || c.id === open);
