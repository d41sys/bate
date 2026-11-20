import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { inShell, listen, request, startDictation } from "./service.ts";
import { add, build, planSteps, type Entry, type Item, type Question, type Row } from "./chat.ts";
import { Markdown } from "./Markdown.ts";
import { canRead, hush, readable, speak } from "./speech.ts";
import { upload } from "./upload.ts";
import type { Bot, QueuedPrompt } from "./types.ts";

type Opened = { history: Row[]; more: boolean; questions: Question[]; queue: QueuedPrompt[] };

/** Entries asked for at a time; a reply is many of them (one per delta). */
const PAGE = 2000;

/** The service's choices, in the person's words. */
const CHOICES: Record<string, string> = { Approve: "Allow once", Always: "Always", Deny: "Deny" };

/** What "Always" remembers: a grant is for this kind of action and exactly this target (PROTOCOL.md, Permissions). */
const COVERS: Record<string, string> = {
  read: "this exact path",
  write: "this exact file",
  run: "this exact command",
  connector: "this connector tool, whatever it is given",
};

type Hit = { seq: number };

/**
 * `jump`: show the item that begins at `seq` (a search hit), loading older pages to reach it.
 * `find`: open the find bar. Each is taken once: `used` says so.
 */
export function Chat({ bot, jump, find = false, used = () => {}, showFile, model, signIn, start = "" }: {
  bot: Bot;
  jump?: { seq: number };
  find?: boolean;
  used?: () => void;
  // Opens a file of the bot's in the pane; absent where there is no pane (other devices).
  showFile?: (path: string) => void;
  // The model and thinking chip, which sits in the message box.
  model?: ReactNode;
  // Set while the bot has no way to reach a model: opens the sign-in, and nothing is sent until then.
  signIn?: () => void;
  // What the message box starts with: a first message to send as it is.
  start?: string;
}) {
  // Prompts the service holds until the bot is free, in the order they go.
  const [waiting, setWaiting] = useState<QueuedPrompt[]>([]);
  const [items, setItems] = useState<Item[]>([]);
  const [questions, setQuestions] = useState<Question[]>([]);
  const [text, setText] = useState(start);
  const [error, setError] = useState("");
  const [compacted, setCompacted] = useState(false);
  const [sending, setSending] = useState(false);
  // State is only read again at the next render; a second click in the same moment must see it now.
  const inFlight = useRef(false);
  const [files, setFiles] = useState<{ id: string; name: string; path?: string; error?: string }[]>([]);
  const picker = useRef<HTMLInputElement>(null);
  const box = useRef<HTMLTextAreaElement>(null);
  // The reply being read aloud.
  const [reading, setReading] = useState<number>();
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
        setWaiting(d.queue);
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
      } else if (frame.type === "queue_changed") {
        request<{ queue: QueuedPrompt[] }>("list_queue", { bot: bot.id }).then(
          (d) => gone || setWaiting(d.queue),
          (e: Error) => gone || setError(e.message),
        );
      } else if (frame.type === "history_changed") {
        open();
      }
    });
    return () => {
      gone = true;
      stop();
    };
  }, [bot.id]);

  // Another bot's chat, or no chat: what is being read stops.
  useEffect(() => () => {
    if (canRead) hush();
    setReading(undefined);
  }, [bot.id]);
  const read = (id: number, text: string) => {
    try {
      speak(text, () => setReading((now) => (now === id ? undefined : now)));
      setReading(id);
    } catch (e) {
      setError((e as Error).message);
    }
  };
  const stopReading = () => {
    hush();
    setReading(undefined);
  };

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
    if (!jump) return;
    used();
    void show(jump.seq);
  }, [jump]);
  useEffect(() => {
    if (!find) return;
    used();
    setFinding(true);
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
  // So does a shorter scroller: the message box grew, or a question came up under it.
  useEffect(() => {
    const el = scroller.current;
    if (!el) return;
    // Through the list: it puts back a position written past it.
    const watch = new ResizeObserver(() => {
      if (atBottom.current) rows.scrollToEnd();
    });
    watch.observe(el);
    return () => watch.disconnect();
  }, []);
  // The message box is as tall as its words, up to the limit the style sets.
  useLayoutEffect(() => {
    const el = box.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${el.scrollHeight}px`;
    // Measuring made the box one line for a moment, which pulled the chat off its end.
    if (atBottom.current) rows.scrollToEnd();
  }, [text]);

  const run = (p: Promise<unknown>) => p.catch((e: Error) => setError(e.message));
  const stop = () => run(request("abort", { bot: bot.id }));
  // One answer per question: a second click or Esc before the service closes it would be refused.
  const answering = useRef(new Set<string>());
  const answer = (q: Question, choice: string) => {
    if (answering.current.has(q.question)) return Promise.resolve();
    answering.current.add(q.question);
    return request("answer", { bot: bot.id, question: q.question, choice }).catch((e) => {
      answering.current.delete(q.question);
      throw e;
    });
  };
  const asked = questions[0];
  // Esc denies the question that waits, else stops a working bot; not when it was spent on something else: a popover, the find bar, the palette.
  // Allowing needs a chord (⌘Enter, taken before the message box can send on it), so a stray Enter cannot approve.
  useEffect(() => {
    if (!bot.busy && !asked) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !e.defaultPrevented && !e.isComposing) run(asked ? answer(asked, "Deny") : stop());
    };
    const onChord = (e: KeyboardEvent) => {
      if (!asked || e.key !== "Enter" || !(e.metaKey || e.ctrlKey) || e.isComposing || !asked.options.includes("Approve")) return;
      e.preventDefault();
      e.stopPropagation();
      run(answer(asked, "Approve"));
    };
    addEventListener("keydown", onKey);
    addEventListener("keydown", onChord, true);
    return () => {
      removeEventListener("keydown", onKey);
      removeEventListener("keydown", onChord, true);
    };
  }, [bot.busy, bot.id, asked?.question]);
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
  const proceed = () => once(() => prompt("Continue."));
  const compact = () =>
    once(async () => {
      await request("compact", { bot: bot.id });
      setCompacted(true);
    });
  /** `steer`: into the turn that runs; otherwise a prompt, which the service queues while the bot is busy. */
  const send = async (steer = false) => {
    // The text stays until the service has it.
    if (!message || uploading || signIn || (bot.busy && editing) || inFlight.current) return;
    inFlight.current = true;
    setSending(true);
    try {
      // A message while a card is open denies it: "not this, do that". The bot reads it as a steer.
      const instead = questions.length > 0 && !bot.agent?.length;
      for (const q of questions) await answer(q, "Deny");
      if (editing) {
        await request("rewind", { bot: bot.id, entry: editing, fork: false });
        // Gone from the chat: a second try only sends.
        setEditing(undefined);
      }
      await (steer || instead ? request("steer", { bot: bot.id, text: message }) : prompt(message));
      setText("");
      setCompacted(false);
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
        <button type="button" className="ghost small" onClick={() => run(request<{ path: string }>("export_chat", { bot: bot.id }).then((d) => setSaved(d.path)))}>
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
                // An installed agent cannot go back in a conversation.
                fixed={!!bot.agent?.length}
                retry={retry}
                proceed={proceed}
                latest={row.index === items.length - 1}
                fork={fork}
                reading={reading === items[row.index].id}
                read={canRead ? read : undefined}
                stopReading={stopReading}
                open={showFile}
                edit={(item) => {
                  setEditing(item.nonce);
                  setText(item.text);
                }}
              />
            </div>
          ))}
        </div>
      </div>
      {questions.map((q, i) => (
        <div key={q.question} className="question" role="group" aria-label={q.title}>
          <p>{q.title}</p>
          {q.target && <pre>{q.target}</pre>}
          {q.folder && <p className="note">In <code>{q.folder}</code></p>}
          <div className="actions">
            {q.options.map((choice) => (
              <button
                key={choice}
                className={choice === "Approve" ? "primary" : choice === "Deny" ? "danger" : undefined}
                onClick={() => run(answer(q, choice))}
              >
                {CHOICES[choice] ?? choice}
                {i === 0 && choice === "Approve" && <kbd>⌘Enter</kbd>}
                {i === 0 && choice === "Deny" && <kbd>Esc</kbd>}
              </button>
            ))}
          </div>
          {q.options.includes("Always") && (
            <p className="note">Always: Bate stops asking for {COVERS[q.kind ?? ""] ?? "this exact action"}. The bot still asks again for anything else. Remove it in Settings.</p>
          )}
          {i === 0 && <p className="note">Writing a message instead denies this and sends the message to the bot.</p>}
        </div>
      ))}
      {error && <p className="error" role="alert">{error}</p>}
      {signIn && (
        <p className="banner" role="status">
          {bot.name} has no model to answer with yet. <button type="button" onClick={signIn}>Sign in</button>
        </p>
      )}
      {compacted && <p className="note" role="status">Done: the bot keeps the last four messages as they are and a summary of anything older. The chat here is unchanged.</p>}
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
      {waiting.length > 0 && (
        <ol className="queue" aria-label="Waiting to be sent">
          {waiting.map((p) => (
            <li key={p.nonce}>
              {p.text}{" "}
              <button type="button" aria-label={`Remove from the queue: ${p.text}`} onClick={() => run(request("remove_queued", { bot: bot.id, nonce: p.nonce }))}>×</button>
            </li>
          ))}
        </ol>
      )}
      <form className="composer" onSubmit={(e) => { e.preventDefault(); send(); }}>
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
        <button type="button" className="round" aria-label="Attach files" data-tip="Attach files" onClick={() => picker.current?.click()}>+</button>
        {inShell && (
          <button
            type="button"
            data-tip="Dictate with the system's Dictation (on this Mac)"
            onClick={() => {
              box.current?.focus();
              run(startDictation());
            }}
          >
            Dictate
          </button>
        )}
        <textarea
          ref={box}
          rows={1}
          aria-label="Message"
          placeholder={`Message ${bot.name}`}
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
        {model}
        {!bot.busy && !bot.agent?.length && (
          <button type="button" disabled={sending} data-tip="Have the bot go on from a summary of the older messages, to make room in what it can hold; the chat here stays whole" onClick={compact}>Compact</button>
        )}
        {bot.busy ? (
          <>
            <button disabled={!message || uploading || sending} data-tip="Send this when the bot is done with what it is doing">Queue</button>
            {bot.agent?.length ? (
              <span className="note">This agent takes no message while it works.</span>
            ) : (
              <button type="button" disabled={!message || uploading || sending} data-tip="Put this into the turn that is running" onClick={() => send(true)}>Steer</button>
            )}
            <button type="button" data-tip={asked ? "Stop the bot" : "Stop the bot (Esc)"} onClick={stop}>Stop {!asked && <kbd>Esc</kbd>}</button>
          </>
        ) : (
          <button className="round" aria-label="Send" data-tip="Send" disabled={!message || uploading || sending || !!signIn}>↑</button>
        )}
      </form>
      {bot.busy && (
        <p className="hints note">
          <span><b>Queue</b> sends it when the bot is done.</span>
          {!bot.agent?.length && <span><b>Steer</b> puts it into the turn now.</span>}
        </p>
      )}
    </section>
  );
}

type Prompt = { nonce: string; text: string };
type Actions = {
  busy: boolean;
  /** No going back from any message. */
  fixed: boolean;
  retry: (p: Prompt) => void;
  /** Goes on after a turn that stopped at its step limit. */
  proceed: () => void;
  /** Whether this is the newest line of the chat. */
  latest: boolean;
  edit: (p: Prompt) => void;
  fork: (p: Prompt) => void;
  open?: (path: string) => void;
  /** Whether this reply is being read aloud; `read` is missing where the page can't speak. */
  reading: boolean;
  read?: (id: number, text: string) => void;
  stopReading: () => void;
};

function Row({ item, busy, fixed, retry, edit, fork, proceed, latest, open, reading, read, stopReading }: { item: Item } & Actions) {
  switch (item.kind) {
    case "user":
      return (
        <>
          <p className={item.past ? "bubble past" : "bubble"}>{item.text}</p>
          {!item.past && !fixed && (
            <div className="actions">
              <button disabled={busy} onClick={() => retry(item)}>Retry</button>
              <button disabled={busy} onClick={() => edit(item)}>Edit</button>
              <button disabled={busy} onClick={() => fork(item)} title="Go on from before this message in a copy; what came after stays here to read">Fork</button>
            </div>
          )}
        </>
      );
    case "reply":
      return <Reply item={item} busy={busy} open={open} reading={reading} read={read} stopReading={stopReading} />;
    case "tool": {
      const steps = item.tool === "plan" && !item.isError ? planSteps(item.args) : undefined;
      if (steps) {
        return (
          <ul className="plan">
            {steps.map((s, i) => (
              <li key={i} aria-current={s.status === "in_progress" ? "step" : undefined}>
                <label><input type="checkbox" checked={s.status === "done"} disabled readOnly /> {s.step}</label>
              </li>
            ))}
          </ul>
        );
      }
      return (
        <details>
          <summary>
            {item.tool} {item.output === undefined ? "…" : item.isError ? "failed" : "done"}
          </summary>
          <pre>{JSON.stringify(item.args, null, 2)}</pre>
          {item.output !== undefined && <pre>{item.output}</pre>}
        </details>
      );
    }
    case "retrying":
      return (
        <p>
          The model did not answer ({item.error}). Trying again in {Math.max(1, Math.round(item.waitMs / 1000))} s (retry {item.attempt} of {item.of}).
        </p>
      );
    case "ended":
      if (item.reason === "limit") {
        return (
          <>
            <p>The turn stopped: {item.error ?? "it reached its step limit"}.</p>
            {latest && (
              <div className="actions">
                <button disabled={busy} onClick={proceed}>Continue</button>
              </div>
            )}
          </>
        );
      }
      return <p>{item.reason === "aborted" ? "Stopped." : `The reply failed: ${item.error ?? item.reason}`}</p>;
    case "steered":
      return <p className="steered">You steered: {item.text}</p>;
    case "forked":
      return <p>Forked here. The bot goes on from before the greyed messages above; they are kept to read.</p>;
    default:
      return <p>{item.text}</p>;
  }
}

function Reply({ item, busy, open, reading, read, stopReading }: {
  item: Extract<Item, { kind: "reply" }>;
  busy: boolean;
  open?: (path: string) => void;
  reading: boolean;
  read?: (id: number, text: string) => void;
  stopReading: () => void;
}) {
  const text = useRef<HTMLDivElement>(null);
  return (
    <>
      <div ref={text} className="bubble">
        <Markdown text={item.text} open={open} />
      </div>
      {read && (
        <div className="actions">
          {reading ? (
            <button onClick={stopReading}>Stop reading</button>
          ) : (
            // A reply still arriving is not read: it would stop at what has come so far.
            <button disabled={busy} onClick={() => text.current && read(item.id, readable(text.current))}>Read aloud</button>
          )}
        </div>
      )}
    </>
  );
}
