import { memo, useEffect, useLayoutEffect, useRef, useState } from "react";
import { listen, request } from "./service.ts";
import { Avatar } from "./Avatar.tsx";
import { Markdown } from "./Markdown.ts";
import { leadOf, mentionAt, withoutMention } from "./room.ts";
import type { Bot, Project } from "./types.ts";

type Line = { seq: number; from: string; to: string; text: string; atMs: number };

// A line is parsed once, not at every key typed in the box.
const Said = memo(({ text }: { text: string }) => <Markdown text={text} />);

/** One conversation with the project's bots: the box works as a bot's own; an "@" picks one bot, no pick goes to the lead. */
export function Room({ project, bots }: { project: Project; bots: Bot[] }) {
  const id = project.id!;
  const [room, setRoom] = useState<Line[]>([]);
  const [text, setText] = useState("");
  // The bot picked from the "@" list: the message is sent to it by id, not by the name's letters.
  const [to, setTo] = useState<string>();
  const [caret, setCaret] = useState(0);
  const [closed, setClosed] = useState(false);
  const [active, setActive] = useState(0);
  const [error, setError] = useState("");
  const inFlight = useRef(false);
  const box = useRef<HTMLTextAreaElement>(null);
  const scroller = useRef<HTMLDivElement>(null);
  const atBottom = useRef(true);
  // Where the caret goes once the box shows its new words.
  const place = useRef<number | undefined>(undefined);

  useEffect(() => {
    const load = () => request<{ room: Line[] }>("list_room", { projectId: id }).then((d) => setRoom(d.room), (e) => setError(e.message));
    load();
    return listen((frame) => {
      if (frame.type === "room_changed" && frame.projectId === id) load();
    });
  }, [id]);

  const members = bots.filter((b) => b.project?.id === id);
  const lead = leadOf(project, members);
  const working = members.filter((b) => b.busy);
  const chosen = members.find((b) => b.id === to);
  const mention = closed ? undefined : mentionAt(text, caret, members);
  const at = mention ? Math.min(active, mention.matches.length - 1) : 0;
  const name = (bot: string) => bots.find((b) => b.id === bot)?.name ?? "(deleted bot)";

  // The list keeps its end in view while new lines come, a bot starts working or the box grows, unless the person scrolled up to read.
  const toEnd = () => {
    const el = scroller.current;
    if (el) el.scrollTop = el.scrollHeight;
  };
  useLayoutEffect(() => {
    if (atBottom.current) toEnd();
  }, [room, working.length]);
  useEffect(() => {
    const el = scroller.current;
    if (!el) return;
    const watch = new ResizeObserver(() => atBottom.current && toEnd());
    watch.observe(el);
    return () => watch.disconnect();
  }, []);
  // The box is as tall as its words, up to the limit the style sets.
  useLayoutEffect(() => {
    const el = box.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${el.scrollHeight}px`;
    if (atBottom.current) toEnd();
    if (place.current !== undefined) {
      el.setSelectionRange(place.current, place.current);
      place.current = undefined;
    }
  }, [text]);

  const send = () => {
    if (!text.trim() || !lead || inFlight.current) return;
    inFlight.current = true;
    atBottom.current = true;
    request("send_room", { projectId: id, text, ...(chosen && { to: chosen.id }) })
      .then(
        () => (setText(""), setTo(undefined), setError("")),
        (e: Error) => setError(e.message),
      )
      .finally(() => (inFlight.current = false));
  };
  const pick = (bot: Bot, start: number) => {
    setText(withoutMention(text, start, caret));
    setTo(bot.id);
    setCaret(start);
    place.current = start;
  };

  return (
    <section className="chat" aria-label={`Room of ${project.name}`}>
      <div
        className="items"
        ref={scroller}
        onScroll={(e) => {
          const el = e.currentTarget;
          atBottom.current = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
        }}
      >
        <div role="log" aria-label="What the bots and you said here">
          {room.map((l) => {
            const from = l.from ? bots.find((b) => b.id === l.from) : undefined;
            return (
              <div key={l.seq} className={`item ${l.from ? "reply" : "user"}`}>
                <small className="from">
                  {from && <Avatar bot={from} />}
                  <b>{l.from ? name(l.from) : "You"}</b>
                  {l.to && ` to ${name(l.to)}`}
                  <time>{new Date(l.atMs).toLocaleTimeString()}</time>
                </small>
                {l.from ? <div className="bubble"><Said text={l.text} /></div> : <p className="bubble">{l.text}</p>}
              </div>
            );
          })}
          {room.length === 0 && <p className="empty">Nothing said here yet.</p>}
        </div>
      </div>
      {error && <p className="error" role="alert">{error}</p>}
      {working.map((b) => (
        <p key={b.id} className="banner actions" role="status">
          <Avatar bot={b} /> {b.name} is working…
          <button type="button" className="small" aria-label={`Stop ${b.name}`} onClick={() => request("abort", { bot: b.id }).catch((e: Error) => setError(e.message))}>Stop</button>
        </p>
      ))}
      <form className="composer" onSubmit={(e) => { e.preventDefault(); send(); }}>
        {mention && (
          <div className="popover start">
            <ul role="listbox" id="room-mentions" aria-label="Bots of this project">
              {mention.matches.map((b, i) => (
                <li key={b.id}>
                  <button
                    type="button"
                    role="option"
                    id={`room-mention-${b.id}`}
                    tabIndex={-1}
                    aria-selected={i === at}
                    className={i === at ? "row plain current" : "row plain"}
                    // The box keeps the focus.
                    onMouseDown={(e) => e.preventDefault()}
                    onClick={() => pick(b, mention.start)}
                  >
                    <Avatar bot={b} />
                    <span className="label"><span>{b.name}</span>{b.busy && <small>Working…</small>}</span>
                  </button>
                </li>
              ))}
            </ul>
          </div>
        )}
        {chosen && (
          <span className="mention">
            <Avatar bot={chosen} />@{chosen.name}
            <button type="button" aria-label={`Do not write to ${chosen.name} only`} onClick={() => { setTo(undefined); box.current?.focus(); }}>×</button>
          </span>
        )}
        <textarea
          ref={box}
          rows={1}
          role="combobox"
          aria-autocomplete="list"
          aria-expanded={!!mention}
          aria-controls="room-mentions"
          aria-activedescendant={mention ? `room-mention-${mention.matches[at].id}` : undefined}
          aria-label="Message"
          placeholder={lead ? `Message ${chosen?.name ?? "the project's bots"}` : "No bot is in this project yet"}
          disabled={!lead}
          value={text}
          onChange={(e) => {
            setText(e.target.value);
            setCaret(e.target.selectionStart);
            setClosed(false);
            setActive(0);
          }}
          onSelect={(e) => setCaret(e.currentTarget.selectionStart)}
          onKeyDown={(e) => {
            if (e.nativeEvent.isComposing) return;
            if (mention) {
              const n = mention.matches.length;
              if (e.key === "ArrowDown" || e.key === "ArrowUp") setActive((at + (e.key === "ArrowDown" ? 1 : n - 1)) % n);
              // ⌃Tab is the next-bot key, not a pick.
              else if (e.key === "Enter" || (e.key === "Tab" && !e.ctrlKey)) pick(mention.matches[at], mention.start);
              else if (e.key === "Escape") setClosed(true);
              else return;
              e.preventDefault();
            } else if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              send();
            } else if (e.key === "Backspace" && !text && chosen) {
              setTo(undefined);
            }
          }}
        />
        <button disabled={!text.trim() || !lead}>Send</button>
      </form>
      <p className="hints note">
        {lead ? (
          <>
            <span>A message with no <b>@</b> goes to {lead.name}, the lead.</span>
            {members.length > 1 && <span>Type <b>@</b> to write to one bot.</span>}
            <span>A bot that is busy gets it when it is free.</span>
          </>
        ) : (
          <span>No bot is in this project yet. Add some on its Bots page.</span>
        )}
      </p>
    </section>
  );
}
