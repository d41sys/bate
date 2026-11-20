/** A note's size as the panel shows it: bytes, then KiB. */
export function size(bytes: number): string {
  return bytes < 1024 ? `${bytes} B` : `${(bytes / 1024).toFixed(1)} KiB`;
}

/** What a note's row is titled with: its first line that has words, without the `#` of a heading. Empty: the row shows the file name. */
export function firstLine(text: string): string {
  const line = text.split("\n").find((l) => l.replace(/^[#\s]+/, "").trim());
  return line ? line.replace(/^[#\s]+/, "").trim() : "";
}

/** The editor: `open` is the note's file name, `""` for a new note, `null` for none; `saved` is the text the service has. */
export type Editor = { open: string | null; name: string; text: string; saved: string };

/** Whether the editor holds what the service does not have. */
export const unsaved = (e: Editor): boolean => (e.open === "" ? e.name !== "" || e.text !== "" : e.open !== null && e.text !== e.saved);

// Typed text outlives the screen: a tab or a bot chosen meanwhile takes the screen away, not the text.
const drafts = new Map<string, Editor>();

/** Keeps the editor of one bot's or project's notes while it holds unsaved text, and lets go of it when it does not. */
export function keep(key: string, e: Editor): void {
  if (unsaved(e)) drafts.set(key, e);
  else drafts.delete(key);
}

/** The unsaved editor kept for `key`, if any. */
export const kept = (key: string): Editor | undefined => drafts.get(key);
