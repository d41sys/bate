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

/** The bots in the order the sidebar shows them: each project's, then those in no project. */
export function shownBots(bots: Bot[], projects: Project[]): Bot[] {
  const lists = sidebar(bots, projects);
  return [...lists.projects.flatMap((p) => p.bots), ...lists.loose];
}

/** The bot `by` places from `from` in `list`, wrapping round; with none open, the first (or the last going back). */
export function stepBot(list: Bot[], from: string | undefined, by: 1 | -1): Bot | undefined {
  const at = list.findIndex((b) => b.id === from);
  return list[at < 0 ? (by > 0 ? 0 : list.length - 1) : (at + by + list.length) % list.length];
}

/** The tab an arrow key (or Home, End) moves to from `tab`, wrapping round; undefined for any other key. */
export function stepTab<T>(tabs: T[], tab: T, key: string): T | undefined {
  const at = tabs.indexOf(tab);
  if (key === "Home") return tabs[0];
  if (key === "End") return tabs[tabs.length - 1];
  if (key === "ArrowLeft") return tabs[(at - 1 + tabs.length) % tabs.length];
  if (key === "ArrowRight") return tabs[(at + 1) % tabs.length];
}

const KEYS: Record<string, string> = { KeyK: "palette", KeyF: "find", KeyN: "new-bot", Comma: "settings", BracketLeft: "previous-bot", BracketRight: "next-bot" };
type Chord = Pick<KeyboardEvent, "code" | "metaKey" | "ctrlKey" | "altKey" | "shiftKey" | "isComposing">;

/**
 * The action a key chord stands for, by the id the actions are kept under. By the physical key, not
 * the letter it typed (Caps Lock). Nothing while an input method composes text. Esc is not here:
 * what it does depends on what is open.
 */
export function shortcut(e: Chord): string | undefined {
  if (e.isComposing || e.altKey) return undefined;
  if (e.metaKey && !e.ctrlKey) {
    if (e.shiftKey) return e.code === "KeyN" ? "new-window" : undefined;
    if (/^Digit[1-9]$/.test(e.code)) return `bot-${e.code[5]}`;
    return KEYS[e.code];
  }
  if (e.ctrlKey && !e.metaKey && e.code === "Tab") return e.shiftKey ? "previous-bot" : "next-bot";
}

export type RowState = "waiting" | "paused" | "working" | "unread" | "idle";

/** The one state a row shows, the most urgent first: waiting for the person, paused, working, unread, idle. */
export const rowState = (bot: Bot, unread: boolean): RowState => (bot.waiting ? "waiting" : bot.paused ? "paused" : bot.busy ? "working" : unread ? "unread" : "idle");

/**
 * Unread: a turn of the bot ended (not stopped by the person) while its chat was not in front of them.
 * It stays so until they look at that chat. Kept by the window, so a new window starts with none.
 */
export function endsUnseen(frame: { type: string; bot?: string; entry?: unknown }, looking: string | undefined): boolean {
  const entry = frame.entry as { type?: string; reason?: string } | undefined;
  return frame.type === "transcript" && !!frame.bot && frame.bot !== looking && entry?.type === "turn_end" && entry.reason !== "aborted";
}
