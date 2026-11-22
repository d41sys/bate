import { useEffect, useRef, useState } from "react";
import { Avatar } from "./Avatar.tsx";
import { add, type Entry, type Item, type Question } from "./chat.ts";
import { Markdown } from "./Markdown.ts";
import { QuestionCard } from "./Question.tsx";
import { answerCommand } from "./secret.ts";
import { answerOf, loadUsed, message, type QuickContext, saveUsed, used, usedFirst } from "./quick.ts";
import { hideQuick, listen, openInBate, request } from "./service.ts";
import type { Bot } from "./types.ts";

/** The conversation the question started: the frames of no other one are the panel's. */
type Asked = { bot: string; conversation: string };

/**
 * Quick Ask: the small panel a global shortcut opens over any app (PROTOCOL.md, "Quick Ask"). A bot, one box, Send;
 * the answer comes in the same panel, with the card of a question the bot raises, and "Open in Bate" goes on in a window.
 * The shell tells the page each time the panel is shown (`bate-quick`): it starts again, empty.
 */
export function QuickAsk() {
  const [bots, setBots] = useState<Bot[]>([]);
  const [botId, setBotId] = useState("");
  const [text, setText] = useState("");
  // What the switches in Settings found when the panel opened: the quote and the picture each can be taken off.
  const [context, setContext] = useState<QuickContext>();
  const [quote, setQuote] = useState<string>();
  const [shot, setShot] = useState<{ on: boolean; window: string }>({ on: false, window: "" });
  const [asked, setAsked] = useState<Asked>();
  const [queued, setQueued] = useState(false);
  const [items, setItems] = useState<Item[]>([]);
  const [questions, setQuestions] = useState<Question[]>([]);
  const [error, setError] = useState("");
  const [sending, setSending] = useState(false);
  // How many times the panel has been shown: each one focuses the box.
  const [shown, setShown] = useState(0);
  const box = useRef<HTMLTextAreaElement>(null);
  const target = useRef<Asked | undefined>(undefined);
  // A late reply to an earlier showing is dropped.
  const round = useRef(0);
  const inFlight = useRef(false);
  const answering = useRef(new Set<string>());

  const start = () => {
    const mine = ++round.current;
    target.current = undefined;
    inFlight.current = false;
    answering.current.clear();
    setText("");
    setContext(undefined);
    setQuote(undefined);
    setShot({ on: false, window: "" });
    setAsked(undefined);
    setQueued(false);
    setItems([]);
    setQuestions([]);
    setError("");
    setSending(false);
    setShown((n) => n + 1);
    const fail = (e: Error) => mine === round.current && setError(e.message);
    request<{ bots: Bot[] }>("list_bots").then((d) => {
      if (mine !== round.current) return;
      setBots(d.bots);
      setBotId((id) => usedFirst(d.bots, loadUsed()).find((b) => b.id === id)?.id ?? usedFirst(d.bots, loadUsed())[0]?.id ?? "");
    }, fail);
    // Slow when the helper has to start: the box is there meanwhile.
    request<QuickContext>("quick_context").then((c) => {
      if (mine !== round.current) return;
      setContext(c);
      setQuote(c.quote ?? undefined);
      setShot({ on: c.windows !== null, window: "" });
    }, fail);
  };

  useEffect(() => {
    const onOpen = () => start();
    addEventListener("bate-quick", onOpen);
    const stop = listen((frame) => {
      if (frame.type === "connected") start();
      else if (frame.type === "bots_changed") request<{ bots: Bot[] }>("list_bots").then((d) => setBots(d.bots), () => {});
      const now = target.current;
      if (!now || frame.bot !== now.bot || (frame.conversation !== undefined && frame.conversation !== now.conversation)) return;
      if (frame.type === "transcript") {
        const entry = frame.entry as Entry;
        setItems((all) => add(all, { seq: frame.seq as number, entry }));
        if (entry.type === "prompt") setQueued(false);
        if (entry.type === "question") setQuestions((qs) => [...qs, entry]);
      } else if (frame.type === "question_closed") {
        setQuestions((qs) => qs.filter((q) => q.question !== frame.question));
      } else if (frame.type === "engine_stopped") {
        setQuestions([]);
      }
    });
    // Esc closes the panel, whatever has the focus; the question, if one is open, stays open in Bate.
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !e.isComposing) void hideQuick();
    };
    addEventListener("keydown", onKey);
    return () => {
      removeEventListener("bate-quick", onOpen);
      removeEventListener("keydown", onKey);
      stop();
    };
  }, []);
  useEffect(() => box.current?.focus(), [shown]);

  const bot = bots.find((b) => b.id === botId);
  const send = async () => {
    if (!bot || !message(text, quote, []) || inFlight.current) return;
    inFlight.current = true;
    setSending(true);
    setError("");
    const mine = round.current;
    try {
      const { conversation } = await request<{ conversation: string }>("new_conversation", { bot: bot.id });
      // Before the prompt: its own entry and the answer come as frames the panel must not miss.
      target.current = { bot: bot.id, conversation };
      const paths: string[] = [];
      if (shot.on && context?.windows) {
        const picture = await request<{ path: string }>("quick_shot", { bot: bot.id, window: shot.window ? Number(shot.window) : null });
        paths.push(picture.path);
      }
      const sent = await request<{ queued?: boolean }>("prompt", { bot: bot.id, conversation, nonce: crypto.randomUUID(), text: message(text, quote, paths) });
      if (mine !== round.current) return;
      saveUsed(used(loadUsed(), bot.id));
      setQueued(!!sent.queued);
      setAsked({ bot: bot.id, conversation });
    } catch (e) {
      if (mine !== round.current) return;
      target.current = undefined;
      setError((e as Error).message);
    } finally {
      if (mine === round.current) {
        inFlight.current = false;
        setSending(false);
      }
    }
  };
  const answer = (q: Question, choice: string, secret?: string) => {
    if (!asked || answering.current.has(q.question)) return;
    answering.current.add(q.question);
    request(...answerCommand(asked.bot, asked.conversation, q, choice, secret)).catch((e: Error) => {
      answering.current.delete(q.question);
      setError(e.message);
    });
  };

  if (asked) {
    const reply = answerOf(items);
    const holder = bots.find((b) => b.id === asked.bot);
    return (
      <section className="quick" aria-label="Quick Ask">
        <header>
          {holder && <Avatar bot={holder} />}
          <strong>{holder?.name}</strong>
          <button className="small" onClick={() => void openInBate(asked.bot).catch((e: Error) => setError(e.message))}>Open in Bate</button>
        </header>
        <p className="asked">{text.trim()}</p>
        <div className="answer" aria-live="polite">
          {reply.text && <div className="bubble"><Markdown text={reply.text} /></div>}
          {queued && <p className="note">The bot is busy with other work. Your question waits its turn.</p>}
          {!reply.ended && !queued && questions.length === 0 && (
            <p className="note"><span className="dots" aria-hidden="true"><i /><i /><i /></span> Working…</p>
          )}
          {reply.ended?.reason === "limit" && <p className="note">The bot stopped at its step limit. Open it in Bate to go on.</p>}
          {reply.ended?.reason === "aborted" && <p className="note">Stopped.</p>}
          {reply.ended?.reason === "error" && <p className="error" role="alert">{reply.ended.error || "The turn failed."}</p>}
        </div>
        {questions[0] && <QuestionCard key={questions[0].question} q={questions[0]} keys={false} answer={(choice, secret) => answer(questions[0], choice, secret)} />}
        {error && <p className="error" role="alert">{error}</p>}
        <small>Esc closes</small>
      </section>
    );
  }
  const windows = context?.windows ?? [];
  return (
    <section className="quick" aria-label="Quick Ask">
      <form
        onSubmit={(e) => {
          e.preventDefault();
          void send();
        }}
      >
        <header>
          {bot && <Avatar bot={bot} />}
          <select aria-label="Ask" value={botId} onChange={(e) => setBotId(e.target.value)}>
            {usedFirst(bots, loadUsed()).map((b) => <option key={b.id} value={b.id}>{b.name}</option>)}
          </select>
        </header>
        <textarea
          ref={box}
          aria-label="Your question"
          placeholder={bot ? `Ask ${bot.name}…` : "Ask a bot…"}
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
              e.preventDefault();
              void send();
            }
          }}
        />
        <ul className="attached">
          {quote && (
            <li>
              <span>Selected text: “{quote.split("\n")[0].slice(0, 60)}{quote.length > 60 || quote.includes("\n") ? "…" : ""}”</span>
              <button type="button" aria-label="Do not send the selected text" onClick={() => setQuote(undefined)}>×</button>
            </li>
          )}
          {shot.on && (
            <li>
              <label>Screenshot of{" "}
                <select value={shot.window} onChange={(e) => setShot({ on: true, window: e.target.value })}>
                  <option value="">the screen</option>
                  {windows.map((w) => <option key={w.id} value={w.id}>{w.app}{w.title ? `: ${w.title}` : ""}</option>)}
                </select>
              </label>
              <button type="button" aria-label="Do not send a screenshot" onClick={() => setShot({ on: false, window: "" })}>×</button>
            </li>
          )}
        </ul>
        {context?.quoteError && <p className="note">Selected text: {context.quoteError}</p>}
        {context?.windowsError && <p className="note">Screenshot: {context.windowsError}</p>}
        {error && <p className="error" role="alert">{error}</p>}
        <div className="foot">
          <small>Enter sends · Esc closes</small>
          <button disabled={!bot || sending || !message(text, quote, [])}>Send</button>
        </div>
      </form>
    </section>
  );
}
