// What the Room works out from the project's bots and the words being typed.
import type { Bot, Project } from "./types.ts";

/** The bot a message with no "@" goes to: the project's lead, else its first bot (as the service decides). */
export function leadOf(project: Project, bots: Bot[]): Bot | undefined {
  const id = project.lead && project.bots.includes(project.lead) ? project.lead : project.bots[0];
  return bots.find((b) => b.id === id);
}

/** The "@" being typed before `caret`, at the start or after a space: where it begins, and the members whose names start with what follows it. */
export function mentionAt(text: string, caret: number, members: Bot[]): { start: number; matches: Bot[] } | undefined {
  const typed = /(?:^|\s)@([^@\n]*)$/.exec(text.slice(0, caret))?.[1];
  if (typed === undefined) return undefined;
  const matches = members.filter((b) => b.name.toLowerCase().startsWith(typed.toLowerCase()));
  return matches.length ? { start: caret - typed.length - 1, matches } : undefined;
}

/** The text with the "@" and what was typed after it taken out (a bot picked becomes a chip, not words). */
export const withoutMention = (text: string, start: number, caret: number) =>
  text.slice(0, start) + text.slice(caret).replace(/^ /, "");
