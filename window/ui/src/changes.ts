// The Changes view: a bot's changed files as diffs, and the comments a person writes on their lines,
// which go back to the bot as one message (PROTOCOL.md, "Changes").

export type ChangedFile = { path: string; status: "added" | "modified" | "deleted" };
export type Listed = { source: "git" | "copies"; git: boolean; files: ChangedFile[]; more: boolean };
/** `text` holds the hunks in `diff`; `binary` and `large` are named, not shown; `none`: nothing to show. */
export type Shown = { kind: "text" | "binary" | "large" | "none"; diff: string };

/** One row of a diff: a hunk's header, or a line that is kept, added or removed, with its numbers in the old and the new file. */
export type Row =
  | { kind: "hunk"; text: string }
  | { kind: "context" | "add" | "del"; text: string; old?: number; now?: number };

const SIGN = { context: " ", add: "+", del: "-" } as const;

type Line = Exclude<Row, { kind: "hunk" }>;
/** The rows of a unified diff, from its first hunk on (the file's header lines say what the list says already). */
export function parseDiff(diff: string): Row[] {
  const rows: Row[] = [];
  let old = 0;
  let now = 0;
  let started = false;
  for (const line of diff.split("\n")) {
    const hunk = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/.exec(line);
    if (hunk) {
      started = true;
      old = Number(hunk[1]);
      now = Number(hunk[2]);
      rows.push({ kind: "hunk", text: line });
    } else if (!started || line.startsWith("\\")) {
      // Before the first hunk, or "\ No newline at end of file".
    } else if (line.startsWith("+")) {
      rows.push({ kind: "add", text: line.slice(1), now: now++ });
    } else if (line.startsWith("-")) {
      rows.push({ kind: "del", text: line.slice(1), old: old++ });
    } else if (line.startsWith(" ")) {
      rows.push({ kind: "context", text: line.slice(1), old: old++, now: now++ });
    }
  }
  return rows;
}

export const countLines = (rows: Row[]) => ({
  added: rows.filter((r) => r.kind === "add").length,
  removed: rows.filter((r) => r.kind === "del").length,
});

/** A comment on lines of one file. It holds the lines it is about, so it stays true when the file changes again. */
export type Comment = {
  id: string;
  path: string;
  /** The numbers are those of the new file, or of the old one when every line is a removed one. */
  side: "new" | "old";
  from: number;
  to: number;
  /** The lines, each with its sign (`+`, `-` or a space), as the diff had them. */
  quote: string;
  text: string;
};

/** The comment of `text` on the rows `from` to `to` (either way round) of `rows`; none when they hold no line. */
export function commentOn(path: string, rows: Row[], from: number, to: number, text: string): Comment | undefined {
  const lines = rows.slice(Math.min(from, to), Math.max(from, to) + 1).filter((r): r is Line => r.kind !== "hunk");
  if (lines.length === 0) return undefined;
  const now = lines.flatMap((r) => (r.now === undefined ? [] : [r.now]));
  const side = now.length > 0 ? "new" : "old";
  const n = side === "new" ? now : lines.flatMap((r) => (r.old === undefined ? [] : [r.old]));
  const quote = lines.map((r) => SIGN[r.kind] + r.text).join("\n");
  return { id: crypto.randomUUID(), path, side, from: Math.min(...n), to: Math.max(...n), quote, text };
}

/** "line 12", "lines 12-14", "removed lines 7-8". */
export function where(c: Pick<Comment, "side" | "from" | "to">): string {
  const noun = c.from === c.to ? "line" : "lines";
  return `${c.side === "old" ? "removed " : ""}${noun} ${c.from === c.to ? c.from : `${c.from}-${c.to}`}`;
}

/** Whether `row` is among the lines the comment is about. */
export function covers(c: Comment, row: Row): boolean {
  if (row.kind === "hunk") return false;
  const n = c.side === "new" ? row.now : row.old;
  return n !== undefined && n >= c.from && n <= c.to && (c.side === "new" || row.kind === "del");
}

/** A code fence longer than any run of backticks in `quote`, so that the lines cannot close it. */
function fence(quote: string): string {
  const longest = Math.max(2, ...(quote.match(/`+/g) ?? []).map((run) => run.length));
  return "`".repeat(longest + 1);
}

/** The one message that sends the comments to the bot: each with its file, its line numbers and the lines it is about. */
export function message(comments: Comment[]): string {
  const parts = comments.map((c, i) => {
    const f = fence(c.quote);
    return `${i + 1}. ${c.path}, ${where(c)}:\n${f}diff\n${c.quote}\n${f}\n${c.text.trim()}`;
  });
  const n = comments.length;
  return `Comments on your changes (${n} ${n === 1 ? "comment" : "comments"}):\n\n${parts.join("\n\n")}`;
}

/** The kept comments of one conversation, so that they outlast the pane and the page. */
const key = (bot: string, conversation: string) => `bate-comments:${bot}:${conversation}`;

export function loadComments(bot: string, conversation: string): Comment[] {
  try {
    const kept: unknown = JSON.parse(localStorage.getItem(key(bot, conversation)) ?? "[]");
    return Array.isArray(kept) ? kept.filter((c): c is Comment => typeof c?.id === "string" && typeof c?.text === "string") : [];
  } catch {
    return [];
  }
}

export function saveComments(bot: string, conversation: string, comments: Comment[]) {
  try {
    if (comments.length === 0) localStorage.removeItem(key(bot, conversation));
    else localStorage.setItem(key(bot, conversation), JSON.stringify(comments));
  } catch {
    // A full or blocked store only means they are not kept past this page.
  }
}
