import type { Bot, Project } from "./types.ts";

/** At most this many bots in a project (the service refuses more). */
export const MAX_BOTS = 20;

/** The last part of a folder's path: the project's name until the person types one. */
export const folderName = (path: string) => path.split("/").filter(Boolean).pop() ?? "";

/** The bots whose name has the words typed, in the order given. */
export const findBots = (bots: Bot[], query: string) => {
  const words = query.trim().toLowerCase();
  return bots.filter((b) => b.name.toLowerCase().includes(words));
};

/** A bot that another project has: it cannot be ticked (a bot is in at most one project). */
export const takenBy = (bot: Bot, project?: Project) => (bot.project && bot.project.id !== project?.id ? bot.project.name : undefined);

/** What `set_project` takes: the output-only field is left out. */
export const sent = (p: Project) => ({ ...p, connectorError: undefined });
