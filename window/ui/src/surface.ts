import type { SurfacePart } from "./types.ts";

/** Items asked for at a time; the service never sends more than 500 whatever is asked. */
export const PART = 100;

const count = (p: SurfacePart) => (p.kind === "table" ? p.rows.length : p.kind === "tree" ? p.nodes.length : p.lines.length);

/** "101 to 200 of 1200". */
export function range(p: SurfacePart): string {
  const n = count(p);
  return n === 0 ? "Nothing here" : `${p.offset + 1} to ${p.offset + n} of ${p.total}`;
}

/** Where the next part begins, if there is one. */
export const nextOffset = (p: SurfacePart): number | undefined => (p.offset + count(p) < p.total ? p.offset + count(p) : undefined);

/** Where the part before begins, if there is one. */
export const prevOffset = (p: SurfacePart): number | undefined => (p.offset > 0 ? Math.max(0, p.offset - PART) : undefined);

/** Where to look instead when the content shrank under the part that was open: its last part. */
export const clampOffset = (p: SurfacePart): number =>
  count(p) === 0 && p.offset > 0 ? Math.max(0, Math.floor((p.total - 1) / PART) * PART) : p.offset;

/** The nodes that show: those not under a collapsed node (the next nodes that are deeper than it). */
export function visibleNodes(nodes: { id: string; depth: number }[], collapsed: ReadonlySet<string>) {
  const shown: { index: number; hasChildren: boolean }[] = [];
  let hiddenBelow = Infinity;
  nodes.forEach((n, index) => {
    if (n.depth > hiddenBelow) return;
    hiddenBelow = Infinity;
    shown.push({ index, hasChildren: (nodes[index + 1]?.depth ?? 0) > n.depth });
    if (collapsed.has(n.id)) hiddenBelow = n.depth;
  });
  return shown;
}
