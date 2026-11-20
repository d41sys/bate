import type { CatalogPreview, Workplace, WorkplacePreview } from "./types.ts";

/** What a workplace needs, one line per kind, as the person is shown it before switching it on. */
export function needsLines(w: Pick<Workplace, "manifest">): string[] {
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

/** A size for the person: `1.5 GiB`, `300 MiB`, `12 KiB`, `5 B`. */
export function sizeText(bytes: number): string {
  const units = ["B", "KiB", "MiB", "GiB"];
  let n = bytes;
  let i = 0;
  while (n >= 1024 && i < units.length - 1) (n /= 1024), i++;
  return `${i === 0 ? n : n.toFixed(1)} ${units[i]}`;
}

/** What the person agrees to before a workplace's downloads start, or `undefined` if it has none. */
export function downloadText(p: WorkplacePreview): string | undefined {
  const n = p.manifest.downloads?.length ?? 0;
  if (n === 0) return undefined;
  return `Installing ${p.manifest.name} downloads ${n === 1 ? "1 file" : `${n} files`}, ${sizeText(p.downloadBytes)} in all, from ${p.hosts.join(", ")}.`;
}

/** How far the downloads are, for the progress line. */
export function progressText(done: number, total: number): string {
  return `Downloading ${sizeText(done)} of ${sizeText(total)} (${Math.floor((done / Math.max(total, 1)) * 100)}%)`;
}

/** What an install or update from the catalog newly needs, one line per kind. */
export function newlyLines(n: CatalogPreview["newly"]): string[] {
  const lines = (
    [
      ["Folders", n.folders],
      ["Network hosts", n.hosts],
      ["Commands", n.commands],
      ["Downloads", n.downloads.map((d) => `${d.dest} (${sizeText(d.size)})`)],
    ] as const
  ).flatMap(([what, items]) => (items.length ? [`${what}: ${items.join(", ")}`] : []));
  return lines.length ? lines : ["Nothing it did not need already."];
}
