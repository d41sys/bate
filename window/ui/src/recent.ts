const STORE = "bate-palette-recent";
export const SHOWN = 5;

/** The key a palette line is remembered by. */
export const keyOf = (c: { group: string; label: string }) => `${c.group}\n${c.label}`;

/** `key` first, then the ones before it without a repeat, at most `SHOWN`. */
export const remember = (keys: string[], key: string) => [key, ...keys.filter((k) => k !== key)].slice(0, SHOWN);

export function loadRecent(): string[] {
  try {
    const keys: unknown = JSON.parse(localStorage.getItem(STORE) ?? "[]");
    return Array.isArray(keys) ? keys.filter((k): k is string => typeof k === "string").slice(0, SHOWN) : [];
  } catch {
    return [];
  }
}

export function saveRecent(keys: string[]) {
  try {
    localStorage.setItem(STORE, JSON.stringify(keys));
  } catch {
    // A full or blocked store only means the list is not kept.
  }
}
