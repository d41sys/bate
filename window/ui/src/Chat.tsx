import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { listen, request } from "./service.ts";
import { add, build, type Entry, type Item, type Question, type Row } from "./chat.ts";
import { SANDBOX, fileUrl } from "./frame.ts";
import { Markdown } from "./Markdown.ts";
import { upload } from "./upload.ts";
import type { Bot } from "./types.ts";

type Opened = { history: Row[]; more: boolean; questions: Question[] };

/** Entries asked for at a time; a reply is many of them (one per delta). */
const PAGE = 2000;

type Hit = { seq: number };

/**
 * `jump`: show the item that begins at `seq` (a search hit), loading older pages to reach it.
 * `find`: bumped to open the find bar.
 */
export function Chat({ bot, jump, find = 0 }: { bot: Bot; jump?: { seq: number }; find?: number }) {
  const [items, setItems] = useState<Item[]>([]);
  const [questions, setQuestions] = useState<Question[]>([]);
  const [text, setText] = useState("");
  const [error, setError] = useState("");
  // A file of the bot's, shown in a frame of its own.
  const [shown, setShown] = useState<string>();
  const [sending, setSending] = useState(false);
  // State is only read again at the next render; a second click in the same moment must see it now.
  const inFlight = useRef(false);
  const [files, setFiles] = useState<{ id: string; name: string; path?: string; error?: string }[]>([]);
  const picker = useRef<HTMLInputElement>(null);
  // The prompt being edited: sending goes back to just before it first.
  const [editing, setEditing] = useState<string>();
  const [saved, setSaved] = useState("");
  // Every entry loaded so far, oldest first; an older page goes in front and the chat is built anew.
  const entries = useRef<Row[]>([]);
  const oldest = useRef<number>(undefined);
  const [more, setMore] = useState(false);
  const older = useRef<() => Promise<boolean>>(() => Promise.resolve(false));
  // The first page, which a jump waits for.
  const opened = useRef<Promise<unknown>>(Promise.resolve());
  const hasMore = useRef(false);
  // The item a jump showed.
  const [marked, setMarked] = useState<number>();
  const [finding, setFinding] = useState(false);
  const [needle, setNeedle] = useState("");
  const [hits, setHits] = useState<Hit[]>();
  const [hit, setHit] = useState(0);
  const scroller = useRef<HTMLDivElement>(null);
  const atBottom = useRef(true);
  const rows = useVirtualizer({
    count: items.length,
    getScrollElement: () => scroller.current,
    estimateSize: () => 48,
    overscan: 8,
    // By item, not place: rows measured stay measured when older ones come in front.
    getItemKey: (index) => items[index].id,
  });

  useEffect(() => {
    let gone = false;
    let loading = false;
    // What arrives while a page is asked for: the page may or may not hold it.
    let live: Row[] | undefined;
    const open = () => {
      live = [];
      return request<Opened>("open_bot", { bot: bot.id, limit: PAGE }).then((d) => {
        if (gone) return;
        const last = d.history[d.history.length - 1]?.seq ?? 0;
        entries.current = [...d.history, ...(live ?? []).filter((row) => row.seq > last)];
        live = undefined;
        oldest.current = d.history[0]?.seq;
        atBottom.current = true;
        hasMore.current = d.more;
        setMore(d.more);
        setItems(build(entries.current));
        setQuestions(d.questions);
      }, (e) => {
        live = undefined;
        setError(e.message);
      });
    };
    older.current = () => {
      if (loading || oldest.current === undefined) return Promise.resolve(false);
      loading = true;
      const before = oldest.current;
      return request<Opened>("open_bot", { bot: bot.id, limit: PAGE, before }).then((d) => {
        loading = false;
        if (gone || before !== oldest.current) return false;
        entries.current = [...d.history, ...entries.current];
        oldest.current = d.history[0]?.seq ?? before;
        hasMore.current = d.more;
        setMore(d.more);
        setItems((shown) => {
          const all = build(entries.current);
          // Keep the rows the user was reading where they were.
          requestAnimationFrame(() => rows.scrollToIndex(all.length - shown.length, { align: "start" }));
          return all;
        });
        return true;
      }, (e) => {
        loading = false;
        setError(e.message);
        return false;
      });
    };
    opened.current = open();
    const stop = listen((frame) => {
      if (frame.bot !== bot.id) return;
      if (frame.type === "transcript") {
        const entry = frame.entry as Entry;
        const row = { seq: frame.seq as number, entry };
        live?.push(row);
        entries.current.push(row);
        setItems((items) => add(items, row));
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

  const show = async (seq: number) => {
    await opened.current;
    while (oldest.current !== undefined && oldest.current > seq && hasMore.current) {
      if (!(await older.current())) break;
    }
    const all = build(entries.current);
    // The item that begins at `seq`, or the one it is part of.
    let index = all.findIndex((item) => item.id >= seq);
    if (index < 0) index = all.length - 1;
    else if (all[index].id > seq && index > 0) index -= 1;
    if (index < 0) return;
    atBottom.current = false;
    setMarked(all[index].id);
    requestAnimationFrame(() => requestAnimationFrame(() => rows.scrollToIndex(index, { align: "center" })));
  };
  useEffect(() => {
    if (jump) void show(jump.seq);
  }, [jump]);
  useEffect(() => {
    if (find) setFinding(true);
  }, [find]);
  // Searched once typing pauses; the newest hit is shown first.
  useEffect(() => {
    if (!finding || !needle.trim()) return setHits(undefined);
    let stale = false;
    const wait = setTimeout(() => {
      request<{ hits: Hit[] }>("search", { bot: bot.id, text: needle, limit: 200 }).then((d) => {
        if (stale) return;
        setHits(d.hits);
        setHit(0);
        if (d.hits[0]) void show(d.hits[0].seq);
      }, (e: Error) => stale || setError(e.message));
    }, 200);
    return () => {
      stale = true;
      clearTimeout(wait);
    };
  }, [needle, finding, bot.id]);
  const step = (by: number) => {
    if (!hits?.length) return;
    const next = (hit + by + hits.length) % hits.length;
    setHit(next);
    void show(hits[next].seq);
  };

  // A reply that grows keeps its end in view, unless the user scrolled up to read.
  useLayoutEffect(() => {
    if (atBottom.current && items.length) rows.scrollToIndex(items.length - 1, { align: "end" });
  }, [items, rows]);

  const run = (p: Promise<unknown>) => p.catch((e: Error) => setError(e.message));
  // One file at a time: the service takes only a few at once from a window.
  const queue = useRef<Promise<unknown>>(Promise.resolve());
  const attach = (list: Iterable<File>) => {
    for (const file of list) {
      const id = crypto.randomUUID();
      setFiles((fs) => [...fs, { id, name: file.name }]);
      queue.current = queue.current.then(() => upload(bot.id, file)).then(
        (path) => setFiles((fs) => fs.map((f) => (f.id === id ? { ...f, path } : f))),
        (e: Error) => setFiles((fs) => fs.map((f) => (f.id === id ? { ...f, error: e.message } : f))),
      );
    }
  };
  const uploading = files.some((f) => !f.path && !f.error);
  const attached = files.flatMap((f) => (f.path ? [`- ${f.path}`] : []));
  const message = [text.trim(), attached.length ? `Attached:\n${attached.join("\n")}` : ""].filter(Boolean).join("\n\n");
  /** One change to the chat at a time; what it was is kept if it fails. */
  const once = async (work: () => Promise<unknown>) => {
    if (bot.busy || inFlight.current) return;
    inFlight.current = true;
    setSending(true);
    try {
      await work();
      setError("");
    } catch (e) {
      setError((e as Error).message);
    } finally {
      inFlight.current = false;
      setSending(false);
    }
  };
  const prompt = (words: string) => request("prompt", { bot: bot.id, nonce: crypto.randomUUID(), text: words });
  const retry = (item: { nonce: string; text: string }) =>
    once(async () => {
      await request("rewind", { bot: bot.id, entry: item.nonce, fork: false });
      // The message is gone from the chat now: if it cannot be sent again, it is not lost.
      await prompt(item.text).catch((e) => {
        setText(item.text);
        throw e;
      });
    });
  const fork = (item: { nonce: string; text: string }) =>
    once(async () => {
      await request("rewind", { bot: bot.id, entry: item.nonce, fork: true });
      setEditing(undefined);
      setText(item.text);
    });
  const send = async () => {
    // The text stays until the service has it.
    if (!message || uploading || bot.busy || inFlight.current) return;
    inFlight.current = true;
    setSending(true);
    try {
      if (editing) {
        await request("rewind", { bot: bot.id, entry: editing, fork: false });
        // Gone from the chat: a second try only sends.
        setEditing(undefined);
      }
      await prompt(message);
      setText("");
      setFiles([]);
      setEditing(undefined);
      setError("");
    } catch (e) {
      setError((e as Error).message);
    } finally {
      inFlight.current = false;
      setSending(false);
    }
  };

  return (
    <section
      className="chat"
      onDragOver={(e) => e.preventDefault()}
      onDrop={(e) => {
        e.preventDefault();
        attach(e.dataTransfer.files);
      }}
    >
      {finding && (
        <div className="find" role="search">
          <input
            autoFocus
            aria-label="Find in this chat"
            placeholder="Find in this chat"
            value={needle}
            onChange={(e) => setNeedle(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") step(e.shiftKey ? -1 : 1);
              else if (e.key === "Escape") setFinding(false);
              else return;
              e.preventDefault();
            }}
          />
          <span className="note" aria-live="polite">
            {hits === undefined ? "" : hits.length ? `${hit + 1} of ${hits.length}${hits.length === 200 ? "+" : ""}` : "Not found"}
          </span>
          <button type="button" aria-label="Newer" disabled={!hits?.length} onClick={() => step(-1)}>↓</button>
          <button type="button" aria-label="Older" disabled={!hits?.length} onClick={() => step(1)}>↑</button>
          <button type="button" onClick={() => setFinding(false)}>Done</button>
        </div>
      )}
      <div className="tools">
        <button type="button" onClick={() => run(request<{ path: string }>("export_chat", { bot: bot.id }).then((d) => setSaved(d.path)))}>
          Export chat
        </button>
        {saved && <span className="note">Saved to <code>{saved}</code></span>}
      </div>
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
              className={`item ${items[row.index].kind}${items[row.index].id === marked ? " marked" : ""}`}
              style={{ position: "absolute", top: 0, left: 0, width: "100%", transform: `translateY(${row.start}px)` }}
            >
              <Row
                item={items[row.index]}
                busy={bot.busy || sending}
                retry={retry}
                fork={fork}
                open={setShown}
                edit={(item) => {
                  setEditing(item.nonce);
                  setText(item.text);
                }}
              />
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
      {(files.length > 0 || editing) && (
        <ul className="attached">
          {editing && (
            <li>
              Editing a message: sending replaces it and what followed.{" "}
              <button type="button" onClick={() => { setEditing(undefined); setText(""); }}>Cancel</button>
            </li>
          )}
          {files.map((f) => (
            <li key={f.id}>
              {f.name} {f.error ? `: ${f.error}` : f.path ? "" : "…"}
              <button type="button" aria-label={`Remove ${f.name}`} onClick={() => setFiles((fs) => fs.filter((x) => x.id !== f.id))}>×</button>
            </li>
          ))}
        </ul>
      )}
      {shown && (
        <div className="shown">
          <div className="tools">
            <code>{shown}</code> <button type="button" onClick={() => setShown(undefined)}>Close</button>
          </div>
          <iframe title={shown} sandbox={SANDBOX} src={fileUrl(bot.id, shown)} />
        </div>
      )}
      <form onSubmit={(e) => { e.preventDefault(); send(); }}>
        <input
          ref={picker}
          type="file"
          multiple
          hidden
          onChange={(e) => {
            attach(e.target.files ?? []);
            e.target.value = "";
          }}
        />
        <button type="button" onClick={() => picker.current?.click()}>Attach</button>
        <textarea
          aria-label="Message"
          value={text}
          onChange={(e) => setText(e.target.value)}
          onPaste={(e) => {
            if (e.clipboardData.files.length) {
              e.preventDefault();
              attach(e.clipboardData.files);
            }
          }}
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
          <button disabled={!message || uploading || sending}>Send</button>
        )}
      </form>
    </section>
  );
}

type Prompt = { nonce: string; text: string };
type Actions = {
  busy: boolean;
  retry: (p: Prompt) => void;
  edit: (p: Prompt) => void;
  fork: (p: Prompt) => void;
  open: (path: string) => void;
};

function Row({ item, busy, retry, edit, fork, open }: { item: Item } & Actions) {
  switch (item.kind) {
    case "user":
      return (
        <>
          <p>{item.text}</p>
          <div className="actions">
            <button disabled={busy} onClick={() => retry(item)}>Retry</button>
            <button disabled={busy} onClick={() => edit(item)}>Edit</button>
            <button disabled={busy} onClick={() => fork(item)} title="Go on from here in a copy; this chat stays as it is">Fork</button>
          </div>
        </>
      );
    case "reply":
      return <Markdown text={item.text} open={open} />;
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
