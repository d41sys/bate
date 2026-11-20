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
