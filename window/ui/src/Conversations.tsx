import { type ReactNode, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { listen, request } from "./service.ts";
import { label, listed, when } from "./conversations.ts";
import type { Conversation } from "./types.ts";

/**
 * A bot's conversations: the one shown, a list to switch, rename, pin and archive them, a new one,
 * export and compact. It shows `children` for the conversation chosen, at first the bot's current one, and
 * stays on it: a conversation made or used on another device does not move the chat under the reader.
 * `want` names one to show (a search hit); `show` switches to another (a fork). The chip that opens the
 * list is drawn into `slot`, a place in the page's header.
 */
export function Conversations({ bot, want, slot, showChanges, children }: {
  bot: string;
  want?: string;
  slot: HTMLElement | null;
  /** Opens the diff of what the bot changed; absent where there is no pane (other devices). */
  showChanges?: (conversation: string) => void;
  children: (conversation: string, show: (id: string) => void, offerCompact: (item: { run: () => void; disabled: boolean } | null) => void) => ReactNode;
}) {
  const [all, setAll] = useState<Conversation[]>([]);
  const [current, setCurrent] = useState<string>();
  const [chosen, setChosen] = useState(want);
  // A new `want` is taken before anything is drawn, so the chat is never opened on the wrong one first.
  const [seen, setSeen] = useState(want);
  if (want !== seen) {
    setSeen(want);
    if (want) setChosen(want);
  }
  const [open, setOpen] = useState(false);
  const [archived, setArchived] = useState(false);
  const [renaming, setRenaming] = useState<{ id: string; name: string }>();
  // The conversation whose actions (Pin, Rename, Archive) are shown.
  const [acting, setActing] = useState<string>();
  const [error, setError] = useState("");
  // Where the last export went.
  const [saved, setSaved] = useState("");
  // Compact, offered by the chat while it can run: it lives in this list because it acts on the shown conversation.
  const [compact, offerCompact] = useState<{ run: () => void; disabled: boolean } | null>(null);
  const wrap = useRef<HTMLDivElement>(null);
  const chip = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    let gone = false;
    const load = () =>
      request<{ conversations: Conversation[]; current: string }>("list_conversations", { bot }).then(
        (d) => {
          if (gone) return;
          setAll(d.conversations);
          setCurrent(d.current);
          // What is shown stays, whatever another device does, until the person switches.
          setChosen((shown) => shown ?? d.current);
        },
        (e: Error) => gone || setError(e.message),
      );
    void load();
    // `bots_changed` also tells when the bot starts or ends a turn: the "Working…" mark.
    const stop = listen((frame) => {
      if (frame.type === "bots_changed" || (frame.type === "conversations_changed" && frame.bot === bot)) void load();
    });
    return () => {
      gone = true;
      stop();
    };
  }, [bot]);

  // While open: a click outside, or Esc, closes it (a row's actions first). Esc is taken here so that it does not also stop the bot.
  useEffect(() => {
    if (!open) return;
    const away = (e: PointerEvent) => {
      if (!wrap.current?.contains(e.target as Node)) setOpen(false);
    };
    const key = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      if (acting) return setActing(undefined);
      setOpen(false);
      chip.current?.focus();
    };
    document.addEventListener("pointerdown", away);
    document.addEventListener("keydown", key);
    return () => {
      document.removeEventListener("pointerdown", away);
      document.removeEventListener("keydown", key);
    };
  }, [open, acting]);

  const shown = chosen ?? current;
  // The last export is of the conversation that was shown then.
  useEffect(() => setSaved(""), [shown]);
  if (!shown) return <p className="note">{error || "Loading the conversations…"}</p>;
  const now = all.find((c) => c.id === shown);

  const run = (work: Promise<unknown>) => work.then(() => setError(""), (e: Error) => setError(e.message));
  const change = (c: Conversation, fields: { pinned?: boolean; archived?: boolean; name?: string }) =>
    run(request("set_conversation", { bot, conversation: c.id, ...fields }));
  const make = () =>
    run(
      request<{ conversation: string }>("new_conversation", { bot }).then((d) => {
        setChosen(d.conversation);
        setOpen(false);
      }),
    );
  const show = (id: string) => {
    setChosen(id);
    setOpen(false);
  };
  /** A button of the list's foot: it closes the list, then does its work. */
  const foot = (words: string, work: () => void, props: { disabled?: boolean; "data-tip"?: string } = {}) => (
    <button
      type="button"
      className="row plain"
      onClick={() => {
        setOpen(false);
        chip.current?.focus();
        work();
      }}
      {...props}
    >
      {words}
    </button>
  );

  const row = (c: Conversation) =>
    renaming?.id === c.id ? (
      <li key={c.id}>
        <form
          onSubmit={(e) => {
            e.preventDefault();
            void change(c, { name: renaming.name.trim() }).then(() => setRenaming(undefined));
          }}
        >
          <input autoFocus aria-label="Name of the conversation" maxLength={80} placeholder={c.title || "A name"} value={renaming.name} onChange={(e) => setRenaming({ id: c.id, name: e.target.value })} />
          <button>Save</button>
          <button type="button" onClick={() => setRenaming(undefined)}>Cancel</button>
        </form>
      </li>
    ) : (
      <li key={c.id} className={acting === c.id ? "acting" : undefined}>
        <button type="button" aria-current={c.id === shown ? "true" : undefined} className={c.id === shown ? "row plain current" : "row plain"} onClick={() => show(c.id)}>
          <span className="label">
            <span>
              {c.pinned && (
                <svg className="pin" viewBox="0 0 24 24" role="img" aria-label="Pinned">
                  <path d="M9 3h6l-1 6 3 3v2H7v-2l3-3-1-6zM12 14v7" />
                </svg>
              )}
              {label(c)}
            </span>
            <small>{c.busy ? "Working…" : when(c.touchedAt)}</small>
          </span>
        </button>
        <button
          type="button"
          className="ghost small more"
          aria-label={`Actions for ${label(c)}`}
          aria-expanded={acting === c.id}
          onClick={() => setActing(acting === c.id ? undefined : c.id)}
        >
          ⋯
        </button>
        {acting === c.id && (
          <span className="actions">
            <button type="button" className="ghost small" onClick={() => void change(c, { pinned: !c.pinned }).then(() => setActing(undefined))}>{c.pinned ? "Unpin" : "Pin"}</button>
            <button type="button" className="ghost small" onClick={() => { setRenaming({ id: c.id, name: c.name }); setActing(undefined); }}>Rename</button>
            <button type="button" className="ghost small" onClick={() => void change(c, { archived: !c.archived }).then(() => setActing(undefined))}>{c.archived ? "Restore" : "Archive"}</button>
          </span>
        )}
      </li>
    );

  return (
    <>
      {slot &&
        createPortal(
          <div className="menu" ref={wrap}>
            <button
              ref={chip}
              type="button"
              className="small chip"
              aria-label={`Conversation: ${now ? label(now) : "…"}. Choose another`}
              aria-haspopup="dialog"
              aria-expanded={open}
              onClick={() => setOpen(!open)}
            >
              <span>{now ? label(now) : "…"}</span> <span aria-hidden="true">▾</span>
            </button>
            {open && (
              <div className="popover below end" role="dialog" aria-label="Conversations of this bot">
                <ul>{listed(all, shown, archived).map(row)}</ul>
                {all.some((c) => c.archived) && (
                  <label>
                    Show archived
                    <input type="checkbox" checked={archived} onChange={(e) => setArchived(e.target.checked)} />
                  </label>
                )}
                <div className="foot">
                  {foot("New conversation", () => void make())}
                  {foot("Export chat", () => void run(request<{ path: string }>("export_chat", { bot, conversation: shown }).then((d) => setSaved(d.path))))}
                  {showChanges && foot("Show changes", () => showChanges(shown), { "data-tip": "What the bot changed in its folder, as a diff to comment on" })}
                  {compact &&
                    foot("Compact this conversation", compact.run, {
                      disabled: compact.disabled,
                      "data-tip": "Have the bot go on from a summary of the older messages, to make room in what it can hold; the chat here stays whole",
                    })}
                </div>
              </div>
            )}
          </div>,
          slot,
        )}
      {error && <p className="error" role="alert">{error}</p>}
      {saved && <p className="note" role="status">Saved to <code>{saved}</code></p>}
      {children(shown, show, offerCompact)}
    </>
  );
}
