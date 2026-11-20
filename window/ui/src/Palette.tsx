import { useEffect, useRef, useState } from "react";
import { Avatar } from "./Avatar.tsx";
import { Dialog } from "./Dialog.tsx";
import { request } from "./service.ts";
import { when } from "./conversations.ts";
import { keyOf, loadRecent, remember, saveRecent } from "./recent.ts";
import type { Bot } from "./types.ts";

/** One line of the palette. Lines of a group sit together; `keys` is the shortcut that does the same; `was` is the group a recent line came from. */
export type Choice = { group: string; label: string; hint?: string; keys?: string; bot?: Bot; was?: string; run: () => void };
type Hit = { bot: string; conversation: string; seq: number; atMs: number; kind: "user" | "reply"; snippet: string };

/**
 * ⌘K: the lines picked last until something is typed; then every command, every bot, and the words in every
 * conversation of every bot that match, in groups. Arrows choose, Enter runs, Escape closes (the dialog's own).
 * `found` runs for a hit.
 */
export function Palette({ choices, bots, found, close }: {
  choices: Choice[];
  bots: Bot[];
  found: (bot: string, conversation: string, seq: number) => void;
  close: () => void;
}) {
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<Hit[]>([]);
  const [searching, setSearching] = useState(false);
  const [failed, setFailed] = useState("");
  const [at, setAt] = useState(0);
  const [recent] = useState(loadRecent);
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    setFailed("");
    if (query.trim().length < 2) return setHits([]), setSearching(false);
    // Searched once typing pauses; an answer to an older query is dropped.
    let stale = false;
    setSearching(true);
    const wait = setTimeout(() => {
      request<{ hits: Hit[] }>("search", { text: query, limit: 20 }).then(
        (d) => stale || (setHits(d.hits), setSearching(false)),
        (e: Error) => stale || (setHits([]), setFailed(e.message), setSearching(false)),
      );
    }, 200);
    return () => {
      stale = true;
      clearTimeout(wait);
    };
  }, [query]);

  const words = query.trim().toLowerCase();
  const find = (id: string) => bots.find((b) => b.id === id);
  // Until something is typed: the lines picked before (or, with none, the commands). Then everything that matches.
  const first = recent.flatMap((k) => {
    const c = choices.find((x) => keyOf(x) === k);
    return c ? [{ ...c, group: "Recent", was: c.group }] : [];
  });
  const shown: Choice[] = words
    ? [
        ...choices.filter((c) => c.label.toLowerCase().includes(words)),
        ...hits.map((h) => ({
          group: "In conversations",
          label: h.snippet,
          hint: `${find(h.bot)?.name ?? h.bot} · ${when(h.atMs)} · ${h.kind === "user" ? "you" : "reply"}`,
          bot: find(h.bot),
          run: () => found(h.bot, h.conversation, h.seq),
        })),
      ]
    : first.length > 0
      ? first
      : choices.filter((c) => c.group === "Commands");
  const chosen = Math.min(at, shown.length - 1);
  // The key moved the choice: keep it in sight.
  useEffect(() => {
    document.getElementById(`palette-${chosen}`)?.scrollIntoView({ block: "nearest" });
  }, [chosen]);
  const groups = shown.reduce<{ name: string; items: { c: Choice; i: number }[] }[]>((all, c, i) => {
    if (all.at(-1)?.name !== c.group) all.push({ name: c.group, items: [] });
    all.at(-1)!.items.push({ c, i });
    return all;
  }, []);
  const pick = (c?: Choice) => {
    if (!c) return;
    if (c.group !== "In conversations") saveRecent(remember(recent, keyOf({ group: c.was ?? c.group, label: c.label })));
    close();
    c.run();
  };

  return (
    <Dialog label="Command palette" className="palette" close={close}>
      <form onSubmit={(e) => { e.preventDefault(); pick(shown[chosen]); }}>
        <input
          ref={input}
          role="combobox"
          aria-label="Search commands, bots and conversations"
          aria-expanded={shown.length > 0}
          aria-controls="palette-list"
          aria-activedescendant={chosen >= 0 ? `palette-${chosen}` : undefined}
          placeholder="Find a command, bot or conversation"
          value={query}
          onChange={(e) => {
            setQuery(e.target.value);
            setAt(0);
          }}
          onKeyDown={(e) => {
            if (e.key === "ArrowDown") setAt((a) => Math.min(a + 1, shown.length - 1));
            else if (e.key === "ArrowUp") setAt((a) => Math.max(a - 1, 0));
            else return;
            e.preventDefault();
          }}
        />
        {failed && <p className="error" role="alert">Search failed: {failed}</p>}
        {shown.length > 0 && (
          <ul id="palette-list" role="listbox" aria-label="Results">
            {groups.map((g) => (
              <li key={g.name} role="group" aria-label={g.name}>
                <small aria-hidden="true">{g.name}</small>
                <ul role="presentation">
                  {g.items.map(({ c, i }) => (
                    <li
                      key={i}
                      id={`palette-${i}`}
                      role="option"
                      className={c.hint || c.bot ? "row" : "row plain"}
                      aria-selected={i === chosen}
                      onMouseEnter={() => setAt(i)}
                      onClick={() => pick(c)}
                    >
                      {c.bot && <Avatar bot={c.bot} />}
                      <span className="label">
                        <span>{c.label}</span>
                        {c.hint && <small>{c.hint}</small>}
                      </span>
                      {c.keys && <kbd>{c.keys}</kbd>}
                    </li>
                  ))}
                </ul>
              </li>
            ))}
          </ul>
        )}
        {searching && <p className="note" role="status">Searching conversations…</p>}
        {!shown.length && !searching && !failed && (
          <div className="empty">
            <p>Nothing matches “{query.trim()}”.</p>
            <button type="button" className="small" onClick={() => (setQuery(""), input.current?.focus())}>Clear the search</button>
          </div>
        )}
      </form>
    </Dialog>
  );
}
