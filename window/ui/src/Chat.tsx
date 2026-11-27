import { useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { inShell, listen, request, startDictation, type Failure } from "./service.ts";
import { add, aim, build, changedFiles, checkText, dayStarts, describe, fold, fullness, fullnessText, given, isLong, WORKING, type Entry, type Fullness, type Item, type Part, type Question, type Row, type Shown } from "./chat.ts";
import { Avatar } from "./Avatar.tsx";
import { KeychainNotice } from "./Keychain.tsx";
import { CopyButton, Markdown } from "./Markdown.ts";
import { QuestionCard } from "./Question.tsx";
import { answerCommand } from "./secret.ts";
import { canRead, hush, readable, speak } from "./speech.ts";
import { clock, day, dayTime, took } from "./times.ts";
import type { Bot, QueuedPrompt } from "./types.ts";
import { ArrowDown, ArrowUp, CalendarClock, Circle, CircleAlert, CircleCheck, CircleDot, Clock, GitFork, Mic, Paperclip, Pencil, Plus, RotateCcw, Square, Volume2, X } from "lucide-react";
import { upload } from "./upload.ts";
import { useLater } from "./useLater.ts";

type Opened = { history: Row[]; more: boolean; questions: Question[]; queue: QueuedPrompt[] };

/** Entries asked for at a time; a reply is many of them (one per delta). */
const PAGE = 2000;

/** How long (ms) a row that just came in counts as arriving: a little longer than its fade (`--slow`). */
const ARRIVING = 400;
/** What an empty conversation offers: a click puts the words in the box and sends nothing. */
const STARTERS: [label: string, words: string][] = [
  ["Plan a task", "Help me plan this task: "],
  ["Explain a file", "Explain what this file does: "],
  ["Review my changes", "Review the changes in this folder and list what could break."],
  ["Write a first draft", "Write a first draft of "],
];

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
  // The conversation whose first page has come: until then an empty chat is not said to be empty.
  const [loaded, setLoaded] = useState<string>();
  // How full the model is, when the engine has said.
  const [filled, setFilled] = useState<Fullness>();
  // The person reads further up: a button brings the latest message back.
  const [away, setAway] = useState(false);
  // Files are held over the chat.
  const [dropping, setDropping] = useState(false);
  // What the person opened or folded by hand: a long message ("Show all"), a run of calls. Kept here, not in the row: a row scrolled far away is drawn anew.
  const [folds, setFolds] = useState<ReadonlyMap<number, boolean>>(new Map());
  // The newest message of the person's that the bot still has: what Retry sends again after a turn that failed or was stopped.
  const lastPrompt = useMemo(() => {
    for (let i = items.length - 1; i >= 0; i--) {
      const item = items[i];
      if (item.kind === "user") return item.past ? undefined : item;
    }
  }, [items]);
  const days = useMemo(() => dayStarts(items), [items]);
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
        setFilled(fullness(entries.current));
        setLoaded(conversation);
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
        const row = { seq: frame.seq as number, at: frame.at as number | undefined, entry };
        live?.push(row);
        entries.current.push(row);
        if (entry.type === "context") setFilled(fullness([row]));
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
      className={dropping ? "chat dropping" : "chat"}
      onDragOver={(e) => {
        e.preventDefault();
        setDropping(e.dataTransfer.types.includes("Files"));
      }}
      onDragLeave={(e) => e.currentTarget.contains(e.relatedTarget as Node | null) || setDropping(false)}
      onDrop={(e) => {
        e.preventDefault();
        setDropping(false);
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
          setAway(!atBottom.current);
          if (more && el.scrollTop < 300) older.current();
        }}
      >
        {loaded === conversation && shown.length === 0 && (
          <div className="empty">
            <Avatar bot={bot} />
            <strong>{bot.name}</strong>
            <span>Nothing is said here yet. Write below to start.</span>
            <div className="prompts">
              {STARTERS.map(([label, words]) => (
                <button key={label} type="button" className="small" onClick={() => { setText(words); box.current?.focus(); }}>{label}</button>
              ))}
            </div>
          </div>
        )}
        <div style={{ height: rows.getTotalSize(), position: "relative" }}>
          {rows.getVirtualItems().map((row) => {
            const item = shown[row.index];
            const before = row.index > 0 ? shown[row.index - 1] : undefined;
            return (
              <div
                key={row.key}
                data-index={row.index}
                ref={rows.measureElement}
                className={`item ${item.kind}${item.id === marked ? " marked" : ""}${Date.now() - (arrived.current.get(item.id) ?? 0) < ARRIVING ? " arriving" : ""}`}
                style={{ position: "absolute", top: 0, left: 0, width: "100%", transform: `translateY(${row.start}px)` }}
              >
                {/* The first thing the bot does after a message of the person's says who, and how long the turn took.
                    The person's own are bubbles at the end edge, with their time under them. */}
                {item.kind !== "user" && (!before || before.kind === "user") && (
                  <small className="from">
                    <Avatar bot={bot} />
                    <b>{bot.name}</b>
                    {before?.kind === "user" && before.took !== undefined && <span>{took(before.took)}</span>}
                  </small>
                )}
                <Row
                  item={item}
                  busy={bot.busy || sending}
                  // The call run that the bot is at now, if it is the last thing said.
                  working={bot.busy && row.index === lastIndex}
                  since={lastPrompt?.took === undefined ? lastPrompt?.began : undefined}
                  firstOfDay={days.has(item.id)}
                  chosen={folds.get(item.id)}
                  setFold={(open) => setFolds((all) => new Map(all).set(item.id, open))}
                  // An installed agent cannot go back in a conversation.
                  fixed={!!bot.agent?.length}
                  retry={retry}
                  again={lastPrompt && !bot.agent?.length ? () => retry(lastPrompt) : undefined}
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
      {away && (
        <div className="jump">
          <button
            type="button"
            className="round"
            aria-label="Go to the latest message"
            data-tip="Go to the latest message"
            onClick={() => {
              atBottom.current = true;
              rows.scrollToIndex(shown.length - 1, { align: "end" });
            }}
          >
            <ArrowDown />
          </button>
        </div>
      )}
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
              <Paperclip />
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
              <Clock />
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
              <CalendarClock />
              <strong>{dayTime(l.dueMs)}</strong> {l.note}{" "}
              <button type="button" aria-label={`Cancel the later turn: ${l.note}`} onClick={() => run(request("cancel_later", { bot: bot.id, later: l.id }))}><X /></button>
            </li>
          ))}
        </ol>
      )}
      <form className="composer" onSubmit={(e) => { e.preventDefault(); send(); }}>
        <textarea
          ref={box}
          rows={1}
          aria-label="Message"
          placeholder={dropping ? "Drop the files here to attach them" : bot.paused ? "Resume the bot to send a message" : bot.busy ? (bot.agent?.length ? "Enter queues it for when the bot is done" : "Enter queues it for when the bot is done, ⌥Enter steers the turn now") : asked ? "Or say what to do instead: that denies the request" : `Message ${bot.name}`}
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
        <span className="end">
          {filled && <Meter filled={filled} />}
          {model}
          {bot.busy ? (
            <button type="button" className="round primary" aria-label="Stop the bot" data-tip={asked ? "Stop the bot" : "Stop the bot (Esc)"} onClick={stop}><Square /></button>
          ) : (
            <button className="round" aria-label="Send" data-tip="Send (Enter)" disabled={!message || uploading || sending || !!signIn || bot.paused}><ArrowUp /></button>
          )}
        </span>
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
  /** Sends the person's newest message again; absent when there is none the bot still has, or no going back. */
  again?: () => void;
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
  /** When the turn that runs began: the working line counts from it. */
  since?: number;
  /** The person's message is the first of its day: its time says the day too. */
  firstOfDay: boolean;
  /** What the person chose for a part that folds (a long message, a run of calls), if anything, and how they choose. */
  chosen?: boolean;
  setFold: (open: boolean) => void;
};

function Row({ item, busy, working, since, firstOfDay, chosen, setFold, fixed, retry, again, edit, fork, proceed, latest, open, changes, bot, reading, read, stopReading }: { item: Shown; working: boolean } & Actions) {
  switch (item.kind) {
    case "user":
      return <Said item={item} busy={busy} fixed={fixed} retry={retry} edit={edit} fork={fork} firstOfDay={firstOfDay} chosen={chosen} setFold={setFold} />;
    case "reply":
      return <Reply item={item} busy={busy} open={open} reading={reading} read={read} stopReading={stopReading} />;
    case "run":
      return <Run parts={item.parts} live={working} bot={bot} changes={changes} chosen={chosen} setFold={setFold} />;
    case "plan":
      return (
        <ul className="plan">
          {item.steps.map((s, i) => {
            const [Mark, state] = s.status === "done" ? [CircleCheck, "Done"] : s.status === "in_progress" ? [CircleDot, "At it now"] : [Circle, "To do"];
            return (
              <li key={i} className={s.status} aria-current={s.status === "in_progress" ? "step" : undefined}>
                <span className="mark"><Mark role="img" aria-label={state} /></span>
                {s.step}
              </li>
            );
          })}
        </ul>
      );
    case "working":
      return (
        <p role="status">
          <span className="dots" aria-hidden="true"><i /><i /><i /></span> Working… {since !== undefined && <Elapsed since={since} />}
        </p>
      );
    case "retrying":
      return (
        <p>
          {working && <span className="mark spin" aria-hidden="true" />}
          The model did not answer ({item.error}). Trying again in {Math.max(1, Math.round(item.waitMs / 1000))} s (retry {item.attempt} of {item.of}).
        </p>
      );
    case "ended": {
      // The newest line only: an older turn's end is past mending from here.
      const retryIt = latest && again && <button className="small" disabled={busy} onClick={again}>Retry</button>;
      if (item.reason === "limit") {
        return (
          <p>
            The turn stopped: {item.error ?? "it reached its step limit"}.
            {latest && <button className="small" disabled={busy} onClick={proceed}>Continue</button>}
          </p>
        );
      }
      if (item.reason === "aborted") return <p>Stopped. {retryIt}</p>;
      return (
        <p className="error">
          <span className="mark" aria-hidden="true"><CircleAlert /></span>
          The reply failed: {item.error ?? item.reason} {retryIt}
        </p>
      );
    }
    case "checked":
      return <p>{checkText(item.result, item.note, item.by)}</p>;
    case "steered":
      return <p className="steered">You steered: {item.text}</p>;
    case "forked":
      return <p>Forked here. The bot goes on from before the greyed messages above; they are kept to read.</p>;
  }
}

/** How long the turn has run, counted each second. Not read out: a screen reader would say every second. */
function Elapsed({ since }: { since: number }) {
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    const tick = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(tick);
  }, []);
  return <span aria-hidden="true">{took(now - since)}</span>;
}

/** How full the model's context is: a ring that fills, with the numbers as its name and tooltip. */
function Meter({ filled }: { filled: Fullness }) {
  const { part, text } = fullnessText(filled);
  return (
    <span className={part >= 0.7 ? "meter near" : "meter"} role="img" aria-label={text} data-tip={text} tabIndex={0}>
      <svg viewBox="0 0 16 16" aria-hidden="true">
        <circle className="track" cx="8" cy="8" r="6" />
        <circle cx="8" cy="8" r="6" pathLength="100" strokeDasharray={`${part * 100} 100`} />
      </svg>
    </span>
  );
}

/** A message of the person's: a long one (a pasted log) is cut until asked for. Under it, when it was said and its actions: Copy and, while the bot can still go back, Retry, Edit and Fork. */
function Said({ item, busy, fixed, retry, edit, fork, firstOfDay, chosen: all = false, setFold }: { item: Extract<Item, { kind: "user" }> } & Pick<Actions, "busy" | "fixed" | "retry" | "edit" | "fork" | "firstOfDay" | "chosen" | "setFold">) {
  const long = isLong(item.text);
  return (
    <>
      <p className={`bubble${item.past ? " past" : ""}${long && !all ? " folded" : ""}`}>{item.text}</p>
      {long && <button type="button" className="small ghost" aria-expanded={all} onClick={() => setFold(!all)}>{all ? "Show less" : "Show all"}</button>}
      <div className="meta">
        <div className="actions">
          <CopyButton label="Copy the message" text={() => item.text} />
          {!item.past && !fixed && (
            <>
              <button aria-label="Retry" data-tip="Retry: send it again" disabled={busy} onClick={() => retry(item)}><RotateCcw /></button>
              <button aria-label="Edit" data-tip="Edit" disabled={busy} onClick={() => edit(item)}><Pencil /></button>
              <button aria-label="Fork" data-tip="Fork: a new conversation from before this message; this one stays as it is" disabled={busy} onClick={() => fork(item)}><GitFork /></button>
            </>
          )}
        </div>
        {item.at !== undefined && <time dateTime={new Date(item.at).toISOString()}>{firstOfDay ? `${day(item.at)} ${clock(item.at)}` : clock(item.at)}</time>}
      </div>
    </>
  );
}

/**
 * Calls and thinking that follow each other: one line saying what happened, with a dot for how it stands, which opens to
 * a row for each call. It is open while the bot is at it and folds when the run is done, unless the person chose.
 */
function Run({ parts, live, bot, changes, chosen, setFold }: { parts: Part[]; live: boolean; bot: string; changes?: () => void } & Pick<Actions, "chosen" | "setFold">) {
  const { text, running, failed } = describe(parts, live);
  const files = changedFiles(parts).length;
  const open = chosen ?? live;
  return (
    <>
      <details className="run" open={open}>
        {/* The click is the person's choice; left to the element, the fold the chat does itself would count as one. */}
        <summary onClick={(e) => { e.preventDefault(); setFold(!open); }}>
          <span className={running ? "dot going" : failed ? "dot bad" : "dot"} aria-hidden="true" />
          {text}
          {failed > 0 && <span className="error">, {failed} failed</span>}
        </summary>
        <ul>
          {parts.map((part) =>
            part.kind === "thinking" ? (
              <li key={part.id} className="thought"><span className="dot" aria-hidden="true" /><p>{part.text}</p></li>
            ) : (
              <li key={part.id}>
                <span className={part.isError ? "dot bad" : live && part.output === undefined ? "dot going" : "dot"} aria-hidden="true" />
                <details>
                  <summary>
                    <code>{aim(part)}</code>
                    {part.isError && <span className="error"> failed</span>}
                  </summary>
                  <pre>{given(part.args)}</pre>
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
      <div className="actions">
        <CopyButton label="Copy the reply" text={() => item.text} />
        {read &&
          (reading ? (
            <button aria-label="Stop reading" data-tip="Stop reading" onClick={stopReading}><Square /></button>
          ) : (
            // A reply still arriving is not read: it would stop at what has come so far.
            <button aria-label="Read aloud" data-tip="Read aloud" disabled={busy} onClick={() => text.current && read(item.id, readable(text.current))}><Volume2 /></button>
          ))}
      </div>
    </>
  );
}
