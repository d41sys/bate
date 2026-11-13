import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { listen, request } from "./service.ts";
import { add, build, type Entry, type Item, type Question } from "./chat.ts";
import { Markdown } from "./Markdown.ts";
import type { Bot } from "./types.ts";

type Opened = { history: { seq: number; entry: Entry }[]; more: boolean; questions: Question[] };

/** Entries asked for at a time; a reply is many of them (one per delta). */
const PAGE = 2000;

export function Chat({ bot }: { bot: Bot }) {
  const [items, setItems] = useState<Item[]>([]);
  const [questions, setQuestions] = useState<Question[]>([]);
  const [text, setText] = useState("");
  const [error, setError] = useState("");
  const [sending, setSending] = useState(false);
  // State is only read again at the next render; a second click in the same moment must see it now.
  const inFlight = useRef(false);
  // Every entry loaded so far, oldest first; an older page goes in front and the chat is built anew.
  const entries = useRef<Entry[]>([]);
  const oldest = useRef<number>(undefined);
  const [more, setMore] = useState(false);
  const older = useRef<() => void>(() => {});
  const scroller = useRef<HTMLDivElement>(null);
  const atBottom = useRef(true);
  const rows = useVirtualizer({
    count: items.length,
    getScrollElement: () => scroller.current,
    estimateSize: () => 48,
    overscan: 8,
  });

  useEffect(() => {
    let gone = false;
    let loading = false;
    const open = () =>
      request<Opened>("open_bot", { bot: bot.id, limit: PAGE }).then((d) => {
        if (gone) return;
        entries.current = d.history.map((h) => h.entry);
        oldest.current = d.history[0]?.seq;
        atBottom.current = true;
        setMore(d.more);
        setItems(build(entries.current));
        setQuestions(d.questions);
      }, (e) => setError(e.message));
    older.current = () => {
      if (loading || oldest.current === undefined) return;
      loading = true;
      const before = oldest.current;
      request<Opened>("open_bot", { bot: bot.id, limit: PAGE, before }).then((d) => {
        loading = false;
        if (gone || before !== oldest.current) return;
        entries.current = [...d.history.map((h) => h.entry), ...entries.current];
        oldest.current = d.history[0]?.seq ?? before;
        setMore(d.more);
        setItems((shown) => {
          const all = build(entries.current);
          // Keep the rows the user was reading where they were.
          requestAnimationFrame(() => rows.scrollToIndex(all.length - shown.length, { align: "start" }));
          return all;
        });
      }, (e) => {
        loading = false;
        setError(e.message);
      });
    };
    open();
    const stop = listen((frame) => {
      if (frame.bot !== bot.id) return;
      if (frame.type === "transcript") {
        const entry = frame.entry as Entry;
        entries.current.push(entry);
        setItems((items) => add(items, entry));
        // History holds every question ever asked; only one asked now is still open.
        if (entry.type === "question") setQuestions((qs) => [...qs, entry]);
      } else if (frame.type === "question_closed") {
        setQuestions((qs) => qs.filter((q) => q.question !== frame.question));
      } else if (frame.type === "engine_stopped") {
        setQuestions([]);
      } else if (frame.type === "history_changed") {
        open();
      }
    });
    return () => {
      gone = true;
      stop();
    };
  }, [bot.id]);

  // A reply that grows keeps its end in view, unless the user scrolled up to read.
  useLayoutEffect(() => {
    if (atBottom.current && items.length) rows.scrollToIndex(items.length - 1, { align: "end" });
  }, [items, rows]);

  const run = (p: Promise<unknown>) => p.catch((e: Error) => setError(e.message));
  const send = async () => {
    const words = text.trim();
    // One prompt at a time; the text stays until the service has it.
    if (!words || bot.busy || inFlight.current) return;
    inFlight.current = true;
    setSending(true);
    try {
      await request("prompt", { bot: bot.id, nonce: crypto.randomUUID(), text: words });
      setText("");
      setError("");
    } catch (e) {
      setError((e as Error).message);
    } finally {
      inFlight.current = false;
      setSending(false);
    }
  };

  return (
    <section className="chat">
      <div
        className="items"
        ref={scroller}
        onScroll={(e) => {
          const el = e.currentTarget;
          atBottom.current = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
          if (more && el.scrollTop < 300) older.current();
        }}
      >
        <div style={{ height: rows.getTotalSize(), position: "relative" }}>
          {rows.getVirtualItems().map((row) => (
            <div
              key={row.key}
              data-index={row.index}
              ref={rows.measureElement}
              className={`item ${items[row.index].kind}`}
              style={{ position: "absolute", top: 0, left: 0, width: "100%", transform: `translateY(${row.start}px)` }}
            >
              <Row item={items[row.index]} />
            </div>
          ))}
        </div>
      </div>
      {questions.map((q) => (
        <div key={q.question} className="question" role="group" aria-label={q.title}>
          <p>{q.title}</p>
          {q.target && <pre>{q.target}</pre>}
          {q.options.map((choice) => (
            <button key={choice} onClick={() => run(request("answer", { bot: bot.id, question: q.question, choice }))}>
              {choice}
            </button>
          ))}
        </div>
      ))}
      {error && <p className="error" role="alert">{error}</p>}
      <form onSubmit={(e) => { e.preventDefault(); send(); }}>
        <textarea
          aria-label="Message"
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
              e.preventDefault();
              send();
            }
          }}
        />
        {bot.busy ? (
          <button type="button" onClick={() => run(request("abort", { bot: bot.id }))}>Stop</button>
        ) : (
          <button disabled={!text.trim() || sending}>Send</button>
        )}
      </form>
    </section>
  );
}

function Row({ item }: { item: Item }) {
  switch (item.kind) {
    case "reply":
      return <Markdown text={item.text} />;
    case "tool":
      return (
        <details>
          <summary>
            {item.tool} {item.output === undefined ? "…" : item.isError ? "failed" : "done"}
          </summary>
          <pre>{JSON.stringify(item.args, null, 2)}</pre>
          {item.output !== undefined && <pre>{item.output}</pre>}
        </details>
      );
    case "ended":
      return <p>{item.reason === "aborted" ? "Stopped." : `The reply failed: ${item.error ?? item.reason}`}</p>;
    default:
      return <p>{item.text}</p>;
  }
}
