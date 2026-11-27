import { useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { inShell, listen, request, startDictation, type Failure } from "./service.ts";
import { add, aim, build, changedFiles, checkText, describe, fold, WORKING, type Entry, type Item, type Part, type Question, type Row, type Shown } from "./chat.ts";
import { Avatar } from "./Avatar.tsx";
import { KeychainNotice } from "./Keychain.tsx";
import { Markdown } from "./Markdown.ts";
import { QuestionCard } from "./Question.tsx";
import { answerCommand } from "./secret.ts";
import { canRead, hush, readable, speak } from "./speech.ts";
import { dayTime } from "./times.ts";
import type { Bot, QueuedPrompt } from "./types.ts";
import { ArrowDown, ArrowUp, Check, Mic, Plus, X } from "lucide-react";
import { upload } from "./upload.ts";
import { useLater } from "./useLater.ts";

type Opened = { history: Row[]; more: boolean; questions: Question[]; queue: QueuedPrompt[] };

/** Entries asked for at a time; a reply is many of them (one per delta). */
const PAGE = 2000;

/** How long (ms) a row that just came in counts as arriving: a little longer than its fade (`--slow`). */
const ARRIVING = 400;

type Hit = { seq: number };

/**
 * `jump`: show the item that begins at `seq` (a search hit), loading older pages to reach it.
 * `find`: open the find bar. Each is taken once: `used` says so.
 */
export function Chat({ bot, conversation, onFork, jump, find = false, used = () => {}, showFile, showChanges, model, signIn, start = "", started = () => {}, offerCompact }: {
  bot: Bot;
  /** The conversation shown; the service holds the rest. */
  conversation: string;
  /** "Fork" made a new conversation: show it. */
  onFork: (conversation: string) => void;
  jump?: { seq: number };
  find?: boolean;
  used?: () => void;
  // Opens a file of the bot's in the pane; absent where there is no pane (other devices).
  showFile?: (path: string) => void;
  // Opens the diff of what the bot changed in the pane; absent where there is no pane.
  showChanges?: () => void;
  // The model and thinking chip, which sits in the message box.
  model?: ReactNode;
  // Set while the bot has no way to reach a model: opens the sign-in, and nothing is sent until then.
  signIn?: () => void;
  // What the message box starts with: a first message to send as it is; `started` says it took it.
  start?: string;
  started?: () => void;
  // Where Compact lives: the conversation chip's list. Offered while the bot can take it, `disabled` while a message goes out.
  offerCompact: (item: { run: () => void; disabled: boolean } | null) => void;
}) {
  // Prompts the service holds until the bot is free, in the order they go.
  const [waiting, setWaiting] = useState<QueuedPrompt[]>([]);
  // Turns the bot set for itself, still waiting for their time.
  const { later } = useLater(bot.id, conversation);
  const [items, setItems] = useState<Item[]>([]);
  const [questions, setQuestions] = useState<Question[]>([]);
  const [text, setText] = useState(start);
  // The first message is the box's start once: not again when another conversation is shown.
  useEffect(() => {
    if (start) started();
  }, []);
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
  // What came in just now fades in; a row scrolled back into view later does not.
  const arrived = useRef(new Map<number, number>());
  // The bot is at work: a mark closes the chat, unless a question is what it waits for.
  const shown = useMemo(() => (bot.busy && questions.length === 0 ? [...fold(items), WORKING] : fold(items)), [items, bot.busy, questions.length]);
  const lastIndex = shown.length - (shown[shown.length - 1] === WORKING ? 2 : 1);
  const rows = useVirtualizer({
    count: shown.length,
    getScrollElement: () => scroller.current,
    estimateSize: () => 48,
    overscan: 8,
    // By item, not place: rows measured stay measured when older ones come in front.
    getItemKey: (index) => shown[index].id,
  });

  // The person reads this conversation: the service stops counting what it holds as not yet seen.
  const looked = () => {
    if (document.hasFocus()) request("mark_seen", { bot: bot.id, conversation }).catch(() => {});
  };
  useEffect(() => {
    addEventListener("focus", looked);
    return () => removeEventListener("focus", looked);
  }, [bot.id, conversation]);

  useEffect(() => {
    let gone = false;
    let loading = false;
    // What arrives while a page is asked for: the page may or may not hold it.
    let live: Row[] | undefined;
    const open = () => {
      live = [];
      return request<Opened>("open_bot", { bot: bot.id, conversation, limit: PAGE }).then((d) => {
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
        looked();
      }, (e) => {
        live = undefined;
        setError(e.message);
      });
    };
    older.current = () => {
      if (loading || oldest.current === undefined) return Promise.resolve(false);
      loading = true;
      const before = oldest.current;
      return request<Opened>("open_bot", { bot: bot.id, conversation, limit: PAGE, before }).then((d) => {
        loading = false;
        if (gone || before !== oldest.current) return false;
        entries.current = [...d.history, ...entries.current];
        oldest.current = d.history[0]?.seq ?? before;
        hasMore.current = d.more;
        setMore(d.more);
        setItems((before) => {
          const all = build(entries.current);
          // Keep the rows the user was reading where they were.
          requestAnimationFrame(() => rows.scrollToIndex(fold(all).length - fold(before).length, { align: "start" }));
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
      // A frame of another conversation of this bot is not this chat's.
      if (frame.bot !== bot.id || (frame.conversation !== undefined && frame.conversation !== conversation)) return;
      if (frame.type === "transcript") {
        const entry = frame.entry as Entry;
        const row = { seq: frame.seq as number, entry };
        live?.push(row);
        entries.current.push(row);
        arrived.current.set(row.seq, Date.now());
        setItems((items) => add(items, row));
        // History holds every question ever asked; only one asked now is still open.
        if (entry.type === "question") setQuestions((qs) => [...qs, entry]);
        if (entry.type === "turn_end") looked();
      } else if (frame.type === "question_closed") {
        setQuestions((qs) => qs.filter((q) => q.question !== frame.question));
      } else if (frame.type === "engine_stopped") {
        setQuestions([]);
      } else if (frame.type === "queue_changed") {
        request<{ queue: QueuedPrompt[] }>("list_queue", { bot: bot.id, conversation }).then(
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
  }, [bot.id, conversation]);

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
    const all = fold(build(entries.current));
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
      request<{ hits: Hit[] }>("search", { bot: bot.id, conversation, text: needle, limit: 200 }).then((d) => {
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
  }, [needle, finding, bot.id, conversation]);
  const step = (by: number) => {
    if (!hits?.length) return;
    const next = (hit + by + hits.length) % hits.length;
    setHit(next);
    void show(hits[next].seq);
  };

  // A reply that grows keeps its end in view, unless the user scrolled up to read.
  useLayoutEffect(() => {
    if (atBottom.current && shown.length) rows.scrollToIndex(shown.length - 1, { align: "end" });
  }, [shown, rows]);
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
  // A narrower box (the side pane opening) wraps its words onto more lines: measure again, or the last ones are cut.
  useEffect(() => {
    const el = box.current;
    if (!el) return;
    let width = el.clientWidth;
    const watch = new ResizeObserver(() => {
      if (el.clientWidth === width) return;
      width = el.clientWidth;
      el.style.height = "auto";
      el.style.height = `${el.scrollHeight}px`;
    });
    watch.observe(el);
    return () => watch.disconnect();
  }, []);

  const run = (p: Promise<unknown>) => p.catch((e: Error) => setError(e.message));
  const stop = () => run(request("abort", { bot: bot.id }));
  // One answer per question: a second click or Esc before the service closes it would be refused.
  const answering = useRef(new Set<string>());
  const answer = (q: Question, choice: string, secret?: string) => {
    if (answering.current.has(q.question)) return Promise.resolve();
    answering.current.add(q.question);
    return request(...answerCommand(bot.id, conversation, q, choice, secret)).catch((e) => {
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
      // A question with no "Deny" among its choices is not answered by Esc: Esc stops the bot then.
      if (e.key === "Escape" && !e.defaultPrevented && !e.isComposing) run(asked?.options.includes("Deny") ? answer(asked, "Deny") : stop());
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
  }, [bot.busy, bot.id, conversation, asked?.question]);
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
  const prompt = (words: string) => request("prompt", { bot: bot.id, conversation, nonce: crypto.randomUUID(), text: words });
  const retry = (item: { nonce: string; text: string }) =>
    once(async () => {
      await request("rewind", { bot: bot.id, conversation, entry: item.nonce, fork: false });
      // The message is gone from the chat now: if it cannot be sent again, it is not lost.
      await prompt(item.text).catch((e) => {
        setText(item.text);
        throw e;
      });
    });
  const fork = (item: { nonce: string; text: string }) =>
    once(async () => {
      const made = await request<{ conversation?: string }>("rewind", { bot: bot.id, conversation, entry: item.nonce, fork: true });
      setEditing(undefined);
      setText(item.text);
      if (made.conversation) onFork(made.conversation);
    });
  const proceed = () => once(() => prompt("Continue."));
  const compact = () =>
    once(async () => {
      await request("compact", { bot: bot.id, conversation });
      setCompacted(true);
    });
  const canCompact = !bot.busy && !bot.agent?.length;
  useEffect(() => {
    offerCompact(canCompact ? { run: () => void compact(), disabled: sending } : null);
    return () => offerCompact(null);
  }, [canCompact, sending]);
  /** `steer`: into the turn that runs; otherwise a prompt, which the service queues while the bot is busy. */
  const send = async (steer = false) => {
    // The text stays until the service has it.
    if (!message || uploading || signIn || bot.paused || (bot.busy && editing) || inFlight.current) return;
    inFlight.current = true;
    setSending(true);
    try {
      // A message while a card is open denies it: "not this, do that". The bot reads it as a steer.
      const instead = questions.length > 0 && !bot.agent?.length;
      for (const q of questions) if (q.options.includes("Deny")) await answer(q, "Deny");
      if (editing) {
        await request("rewind", { bot: bot.id, conversation, entry: editing, fork: false });
        // Gone from the chat: a second try only sends.
        setEditing(undefined);
      }
      await (steer || instead ? request("steer", { bot: bot.id, conversation, text: message }) : prompt(message));
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
            aria-label="Find in this conversation"
            placeholder="Find in this conversation"
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
          <button type="button" aria-label="Newer" disabled={!hits?.length} onClick={() => step(-1)}><ArrowDown /></button>
          <button type="button" aria-label="Older" disabled={!hits?.length} onClick={() => step(1)}><ArrowUp /></button>
          <button type="button" onClick={() => setFinding(false)}>Done</button>
        </div>
      )}
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
          {rows.getVirtualItems().map((row) => {
            const item = shown[row.index];
            return (
              <div
                key={row.key}
                data-index={row.index}
                ref={rows.measureElement}
                className={`item ${item.kind}${item.id === marked ? " marked" : ""}${Date.now() - (arrived.current.get(item.id) ?? 0) < ARRIVING ? " arriving" : ""}`}
                style={{ position: "absolute", top: 0, left: 0, width: "100%", transform: `translateY(${row.start}px)` }}
              >
                {/* A work log: each of the person's messages, and the first thing the bot does after one, says who. */}
                {item.kind === "user" ? (
                  <small className="from"><b>You</b></small>
                ) : (
                  (row.index === 0 || shown[row.index - 1].kind === "user") && <small className="from"><Avatar bot={bot} /><b>{bot.name}</b></small>
                )}
                <Row
                  item={item}
                  busy={bot.busy || sending}
                  // The call run that the bot is at now, if it is the last thing said.
                  working={bot.busy && row.index === lastIndex}
                  // An installed agent cannot go back in a conversation.
                  fixed={!!bot.agent?.length}
                  retry={retry}
                  proceed={proceed}
                  latest={row.index === lastIndex}
                  fork={fork}
                  reading={reading === item.id}
                  read={canRead ? read : undefined}
                  stopReading={stopReading}
                  open={showFile}
                  changes={showChanges}
                  bot={bot.id}
                  edit={(item) => {
                    setEditing(item.nonce);
                    setText(item.text);
                  }}
                />
              </div>
            );
          })}
        </div>
      </div>
      {questions.map((q, i) => (
        <QuestionCard key={q.question} q={q} keys={i === 0} answer={(choice, secret) => run(answer(q, choice, secret))} />
      ))}
      {error && <p className="error" role="alert">{error}</p>}
      <KeychainNotice />
      {signIn && (
        <p className="banner" role="status">
          {bot.name} has no model to answer with yet. <button type="button" onClick={signIn}>Sign in</button>
        </p>
      )}
      {bot.paused && (
        <p className="banner actions" role="status">
          {bot.name} is paused.
          <button type="button" className="small" onClick={() => run(request("set_paused", { bot: bot.id, paused: false }))}>Resume</button>
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
              <button type="button" aria-label={`Remove ${f.name}`} onClick={() => setFiles((fs) => fs.filter((x) => x.id !== f.id))}><X /></button>
            </li>
          ))}
        </ul>
      )}
      {waiting.length > 0 && (
        <ol className="queue" aria-label="Waiting to be sent">
          {waiting.map((p) => (
            <li key={p.nonce}>
              {p.text}{" "}
              <button type="button" aria-label={`Remove from the queue: ${p.text}`} onClick={() => run(request("remove_queued", { bot: bot.id, conversation, nonce: p.nonce }))}><X /></button>
            </li>
          ))}
        </ol>
      )}
      {later.length > 0 && (
        <ol className="queue" aria-label="Later turns">
          {later.map((l) => (
            <li key={l.id}>
              <strong>{dayTime(l.dueMs)}</strong> {l.note}{" "}
              <button type="button" aria-label={`Cancel the later turn: ${l.note}`} onClick={() => run(request("cancel_later", { bot: bot.id, later: l.id }))}><X /></button>
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
        <button type="button" className="round" aria-label="Attach files" data-tip="Attach files" onClick={() => picker.current?.click()}><Plus /></button>
        {inShell && (
          <button
            type="button"
            className="round"
            aria-label="Dictate"
            data-tip="Dictate with the system's Dictation (on this Mac)"
            onClick={() => {
              box.current?.focus();
              run(startDictation());
            }}
          >
            <Mic />
          </button>
        )}
        <textarea
          ref={box}
          rows={1}
          aria-label="Message"
          placeholder={bot.paused ? "Resume the bot to send a message" : bot.busy ? (bot.agent?.length ? "Enter queues it for when the bot is done" : "Enter queues it for when the bot is done, ⌥Enter steers the turn now") : asked ? "Or say what to do instead: that denies the request" : `Message ${bot.name}`}
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
              // While the bot works, Enter queues the message and ⌥Enter puts it into the turn that runs.
              send(e.altKey && bot.busy && !bot.agent?.length);
            }
          }}
        />
        {model}
        {bot.busy ? (
          <button type="button" className="primary" data-tip={asked ? "Stop the bot" : "Stop the bot (Esc)"} onClick={stop}>Stop</button>
        ) : (
          <button disabled={!message || uploading || sending || !!signIn || bot.paused}>Send</button>
        )}
      </form>
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
  /** Opens the diff of what the bot changed. */
  changes?: () => void;
  /** The bot's id: what Undo names. */
  bot: string;
  /** Whether this reply is being read aloud; `read` is missing where the page can't speak. */
  reading: boolean;
  read?: (id: number, text: string) => void;
  stopReading: () => void;
};

function Row({ item, busy, working, fixed, retry, edit, fork, proceed, latest, open, changes, bot, reading, read, stopReading }: { item: Shown; working: boolean } & Actions) {
  switch (item.kind) {
    case "user":
      return (
        <>
          <p className={item.past ? "bubble past" : "bubble"}>{item.text}</p>
          {!item.past && !fixed && (
            <div className="actions">
              <button disabled={busy} onClick={() => retry(item)}>Retry</button>
              <button disabled={busy} onClick={() => edit(item)}>Edit</button>
              <button disabled={busy} onClick={() => fork(item)} title="Start a new conversation from before this message; this one stays as it is">Fork</button>
            </div>
          )}
        </>
      );
    case "reply":
      return <Reply item={item} busy={busy} open={open} reading={reading} read={read} stopReading={stopReading} />;
    case "run":
      return <Run parts={item.parts} live={working} bot={bot} changes={changes} />;
    case "plan":
      return (
        <ul className="plan">
          {item.steps.map((s, i) => (
            <li key={i} aria-current={s.status === "in_progress" ? "step" : undefined}>
              <label><input type="checkbox" checked={s.status === "done"} disabled readOnly /> {s.step}</label>
            </li>
          ))}
        </ul>
      );
    case "working":
      return (
        <p role="status">
          <span className="dots" aria-hidden="true"><i /><i /><i /></span> Working…
        </p>
      );
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
    case "checked":
      return <p>{checkText(item.result, item.note, item.by)}</p>;
    case "steered":
      return <p className="steered">You steered: {item.text}</p>;
    case "forked":
      return <p>Forked here. The bot goes on from before the greyed messages above; they are kept to read.</p>;
  }
}

/** Calls and thinking that follow each other: one line saying what happened, which opens to a row for each call. */
function Run({ parts, live, bot, changes }: { parts: Part[]; live: boolean; bot: string; changes?: () => void }) {
  const { text, running, failed } = describe(parts, live);
  const files = changedFiles(parts).length;
  return (
    <>
      <details className="run">
        <summary>
          <span className={running ? "mark spin" : failed ? "mark bad" : "mark ok"} aria-hidden="true">{!running && (failed ? <X /> : <Check />)}</span>
          {text}
          {failed > 0 && <span className="error">, {failed} failed</span>}
        </summary>
        <ul>
          {parts.map((part) =>
            part.kind === "thinking" ? (
              <li key={part.id} className="thought"><p>{part.text}</p></li>
            ) : (
              <li key={part.id}>
                <details>
                  <summary>
                    <code>{aim(part)}</code>
                    {part.isError && <span className="error"> failed</span>}
                  </summary>
                  <pre>{JSON.stringify(part.args, null, 2)}</pre>
                  {part.output !== undefined && <pre>{part.output}</pre>}
                </details>
                {inShell && <Undo part={part} bot={bot} />}
              </li>
            ),
          )}
        </ul>
      </details>
      {changes && !live && files > 0 && (
        <p className="changed">
          Changed {files === 1 ? "1 file" : `${files} files`}
          <button type="button" className="small ghost" onClick={changes}>See changes</button>
        </p>
      )}
    </>
  );
}

/**
 * What a write or edit that kept the earlier file offers at the end of its line: Undo, then "Undone". A file that is no
 * longer as the change left it is not overwritten: the line says so and offers "Undo anyway".
 */
function Undo({ part, bot }: { part: Extract<Part, { kind: "tool" }>; bot: string }) {
  const [state, setState] = useState<{ busy?: boolean; error?: string; changed?: boolean }>({});
  if (!part.undo || part.isError) return null;
  if (part.undone) return <span className="undo"><small className="note" role="status">Undone</small></span>;
  const run = (force: boolean) => {
    setState({ busy: true });
    request("undo_change", { bot, change: part.undo, force }).then(
      () => setState({}),
      (e: Failure) => setState(e.code === "changed" ? { changed: true } : { error: e.message }),
    );
  };
  return (
    <span className="undo">
      {state.changed && <small className="error" role="alert">No longer as the bot left it.</small>}
      {state.error && <small className="error" role="alert">{state.error}</small>}
      {state.changed ? (
        <button className="small danger" disabled={state.busy} onClick={() => run(true)}>Undo anyway</button>
      ) : (
        <button className="small" disabled={state.busy} onClick={() => run(false)}>Undo</button>
      )}
    </span>
  );
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
