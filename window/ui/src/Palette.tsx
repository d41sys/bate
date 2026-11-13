import { useEffect, useRef, useState } from "react";
import { request } from "./service.ts";
import type { Bot } from "./types.ts";

export type Choice = { label: string; hint?: string; run: () => void };
type Hit = { bot: string; seq: number; kind: "user" | "reply"; snippet: string };

/**
 * ⌘K: every command, every bot, and what the bots' chats say. Arrows choose, Enter runs,
 * Escape closes. `found` runs for a chat hit.
 */
export function Palette({ choices, bots, found, close }: {
  choices: Choice[];
  bots: Bot[];
  found: (bot: string, seq: number) => void;
  close: () => void;
}) {
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<Hit[]>([]);
  const [at, setAt] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    // Focus comes back where it was when the palette closes.
    const before = document.activeElement as HTMLElement | null;
    input.current?.focus();
    return () => before?.focus();
  }, []);
  useEffect(() => {
    if (query.trim().length < 2) return setHits([]);
    // Searched once typing pauses; an answer to an older query is dropped.
    let stale = false;
    const wait = setTimeout(() => {
      request<{ hits: Hit[] }>("search", { text: query, limit: 20 }).then((d) => stale || setHits(d.hits), () => stale || setHits([]));
    }, 200);
    return () => {
      stale = true;
      clearTimeout(wait);
    };
  }, [query]);

  const words = query.trim().toLowerCase();
  const name = (id: string) => bots.find((b) => b.id === id)?.name ?? id;
  const shown: Choice[] = [
    ...choices.filter((c) => c.label.toLowerCase().includes(words)),
    ...hits.map((h) => ({
      label: h.snippet,
      hint: `${name(h.bot)} · ${h.kind === "user" ? "you" : "reply"}`,
      run: () => found(h.bot, h.seq),
    })),
  ];
  const chosen = Math.min(at, shown.length - 1);
  const pick = (c?: Choice) => {
    if (!c) return;
    close();
    c.run();
  };

  return (
    <div className="palette" role="dialog" aria-modal="true" aria-label="Command palette" onClick={close}>
      <div onClick={(e) => e.stopPropagation()}>
        <input
          ref={input}
          role="combobox"
          aria-expanded="true"
          aria-controls="palette-list"
          aria-activedescendant={chosen >= 0 ? `palette-${chosen}` : undefined}
          placeholder="A command, a bot, or words from a chat"
          value={query}
          onChange={(e) => {
            setQuery(e.target.value);
            setAt(0);
          }}
          onKeyDown={(e) => {
            if (e.key === "ArrowDown") setAt((a) => Math.min(a + 1, shown.length - 1));
            else if (e.key === "ArrowUp") setAt((a) => Math.max(a - 1, 0));
            else if (e.key === "Enter") pick(shown[chosen]);
            else if (e.key === "Escape") close();
            // The input is all there is to focus here: Tab stays in the palette.
            else if (e.key === "Tab") {}
            else return;
            e.preventDefault();
          }}
        />
        <ul id="palette-list" role="listbox">
          {shown.map((c, i) => (
            <li
              key={i}
              id={`palette-${i}`}
              role="option"
              aria-selected={i === chosen}
              onMouseEnter={() => setAt(i)}
              onClick={() => pick(c)}
            >
              {c.label} {c.hint && <small>{c.hint}</small>}
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}
