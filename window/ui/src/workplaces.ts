import type { Workplace } from "./types.ts";

/** What a workplace needs, one line per kind, as the person is shown it before switching it on. */
export function needsLines(w: Workplace): string[] {
  const n = w.manifest?.needs;
  const lines = (
    [
      ["Folders", n?.folders],
      ["Network hosts", n?.hosts],
      ["Commands", n?.commands],
    ] as const
  ).flatMap(([what, items]) => (items?.length ? [`${what}: ${items.join(", ")}`] : []));
  return lines.length ? lines : ["Nothing beyond its own folder."];
}

/** Why a workplace cannot be switched on now, if it cannot. */
export function unusable(w: Workplace): string | undefined {
  if (w.error) return w.error;
  if (w.changed) return "its files changed since they were approved";
  return undefined;
}
