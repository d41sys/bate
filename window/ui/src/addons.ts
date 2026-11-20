import type { Connector } from "./types.ts";

/** Who made an add-on, as far as Bate can say, shown before it is installed. */
export const madeBy = {
  catalog: "Bate's catalog, a list in Bate's own repository on GitHub (d41sys/bate). Each entry pins the exact archive by its hash.",
  folder: "A folder you chose on this Mac. Bate cannot tell who made it.",
  typed: "You, here. Bate cannot tell who runs the server or the program.",
  copy: (bot: string) => `${bot}'s copy. Bate cannot tell who made it.`,
};

/** One add-on and the bots that have it, each with its own copy's data. */
export type Placed<T> = { name: string; users: { bot: string; item: T }[] };

/** Groups what each bot has by `same` (the name, unless the bots' copies must also match), sorted by name. */
export function placed<T extends { name: string }>(lists: { bot: string; items: T[] }[], same: (item: T) => string = (i) => i.name): Placed<T>[] {
  const rows = new Map<string, Placed<T>>();
  for (const { bot, items } of lists) {
    for (const item of items) {
      const key = same(item);
      const row = rows.get(key) ?? { name: item.name, users: [] };
      row.users.push({ bot, item });
      rows.set(key, row);
    }
  }
  return [...rows.entries()].sort(([a, x], [b, y]) => x.name.localeCompare(y.name) || a.localeCompare(b)).map(([, row]) => row);
}

/** Runs `run` for each id in turn, so one failure does not stop the others: the ones that failed, and why. */
export async function each(ids: string[], run: (id: string) => Promise<unknown>) {
  const failed: { id: string; error: string }[] = [];
  for (const id of ids) await run(id).catch((e: Error) => failed.push({ id, error: e.message }));
  return failed;
}

/** What a connector's line says: off, waiting for the sign-in, failed to load (and why), or no problem reported. */
export function connectorState(c: Connector): { text: string; problem: boolean } {
  if (!c.enabled) return { text: "Off", problem: false };
  if (c.error) return { text: `Could not start: ${c.error}`, problem: true };
  if (c.oauth && !c.signedIn) return { text: "On, but not signed in", problem: true };
  return { text: "On, no problem reported", problem: false };
}

/** What a connector may do, one line each, as shown before it is added. */
export function connectorMay(c: Connector): string[] {
  const t = c.transport;
  return [
    t.type === "http"
      ? `The bot calls the server at ${t.url}: what it sends a tool goes there.${c.oauth ? " The server signs you in." : ""}`
      : `Runs ${[t.command, ...(t.args ?? [])].join(" ")} on this Mac, in the bot's folder, while the bot works.`,
    "Every tool it offers asks you first, until you answer Always for that tool.",
  ];
}

export type ConnectorForm = { name: string; type: "http" | "stdio"; url: string; command: string; args: string; oauth: boolean };

/** The connector a form describes: each line of the arguments is one argument, so none is split at a space. */
export function connectorOf(f: ConnectorForm): Connector {
  return {
    name: f.name.trim(),
    enabled: true,
    transport:
      f.type === "http"
        ? { type: "http", url: f.url.trim() }
        : { type: "stdio", command: f.command.trim(), args: f.args.split("\n").filter(Boolean) },
    oauth: f.type === "http" && f.oauth,
  };
}

/** Every word of `query` is in one of the texts, in any case; no query matches all. */
export const matches = (query: string, ...texts: string[]) => {
  const hay = texts.join("\n").toLowerCase();
  return query.toLowerCase().split(/\s+/).filter(Boolean).every((w) => hay.includes(w));
};
