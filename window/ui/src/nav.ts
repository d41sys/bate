import type { Bot, Project } from "./types.ts";

/** The sidebar's two lists, both in the bots' order: each project with its bots, and the bots in no project. */
export function sidebar(bots: Bot[], projects: Project[]) {
  return {
    projects: projects.map((project) => ({ project, bots: bots.filter((b) => b.project?.id === project.id) })),
    loose: bots.filter((b) => !b.project),
  };
}

/** The bot `by` places from `id` among the bots shown with it (its project's, or those in no project). */
export function neighbour(bots: Bot[], id: string, by: number): Bot | undefined {
  const bot = bots.find((b) => b.id === id);
  if (!bot) return undefined;
  const shown = bots.filter((b) => b.project?.id === bot.project?.id);
  return shown[shown.indexOf(bot) + by];
}

export type RowState = "waiting" | "working" | "unread" | "idle";

/** The one state a row shows, the most urgent first: waiting for the person, working, unread, idle. */
export const rowState = (bot: Bot, unread: boolean): RowState => (bot.waiting ? "waiting" : bot.busy ? "working" : unread ? "unread" : "idle");

/** How many bots wait for the person: the Dock's badge. */
export const waitingCount = (bots: Bot[]) => bots.filter((b) => b.waiting).length;

/**
 * Unread: a turn of the bot ended (not stopped by the person) while its chat was not in front of them.
 * It stays so until they look at that chat. Kept by the window, so a new window starts with none.
 */
export function endsUnseen(frame: { type: string; bot?: string; entry?: unknown }, looking: string | undefined): boolean {
  const entry = frame.entry as { type?: string; reason?: string } | undefined;
  return frame.type === "transcript" && !!frame.bot && frame.bot !== looking && entry?.type === "turn_end" && entry.reason !== "aborted";
}
