import { useEffect, useRef, useState } from "react";
import { listen, request } from "./service.ts";
import { Avatar } from "./Avatar.tsx";
import { QuestionCard } from "./Question.tsx";
import { ago, answered, grouped, said, type AttentionItem } from "./attention.ts";
import type { Question } from "./chat.ts";
import type { Bot } from "./types.ts";

/** Frames after which the list may differ. */
const CHANGES = ["bots_changed", "conversations_changed", "question_closed"];

/**
 * Every conversation that is not idle, one card each, in groups: what needs the person, what works, what
 * ended since they last looked, what failed. A question is answered on its card; the card opens the conversation.
 */
export function Attention({ bots, open }: { bots: Bot[]; open: (bot: string, conversation: string) => void }) {
  const [items, setItems] = useState<AttentionItem[]>();
  const [error, setError] = useState("");
  // The clock for "5 min ago".
  const [now, setNow] = useState(Date.now);
  // One answer per question: a second click before the service closes it would be refused.
  const answering = useRef(new Set<string>());

  useEffect(() => {
    let gone = false;
    let latest = 0;
    const load = () => {
      const mine = ++latest;
      request<{ items: AttentionItem[] }>("list_attention").then(
        (d) => mine === latest && !gone && (setItems(d.items), setError("")),
        (e: Error) => gone || setError(e.message),
      );
    };
    load();
    const stop = listen((frame) => {
      const entry = frame.entry as { type?: string } | undefined;
      if (CHANGES.includes(frame.type) || (frame.type === "transcript" && (entry?.type === "turn_end" || entry?.type === "question"))) load();
    });
    const tick = setInterval(() => setNow(Date.now()), 30_000);
    return () => {
      gone = true;
      stop();
      clearInterval(tick);
    };
  }, []);

  const answer = (i: AttentionItem, q: Question, choice: string) => {
    if (answering.current.has(q.question)) return;
    answering.current.add(q.question);
    request("answer", { bot: i.bot, conversation: i.conversation, question: q.question, choice }).then(
      () => setItems((all) => all && answered(all, i.conversation, q.question)),
      (e: Error) => {
        answering.current.delete(q.question);
        setError(e.message);
      },
    );
  };

  const groups = grouped(items ?? []);
  return (
    <>
      {error && <p className="error" role="alert">{error}</p>}
      {!items && !error && <p className="note">Loading…</p>}
      {items && groups.length === 0 && <div className="empty"><p>Nothing needs you, and no bot is working.</p></div>}
      {groups.map((g) => (
        <section key={g.id} className="group" aria-labelledby={`attention-${g.id}`}>
          <h2 id={`attention-${g.id}`}>{g.title}</h2>
          <ul className="attention">
            {g.items.map((i) => {
              const bot = bots.find((b) => b.id === i.bot);
              return (
                bot && (
                  <li key={i.conversation} className="card">
                    <button type="button" className="row open" onClick={() => open(i.bot, i.conversation)}>
                      <Avatar bot={bot} />
                      <span className="label">
                        <span>{bot.name}</span>
                        <small>{i.title || "Untitled"}</small>
                        {g.id !== "needs" && <span className="line">{said(i)}</span>}
                      </span>
                      <small className="when">{ago(i.atMs, now)}</small>
                    </button>
                    {i.questions.map((q) => <QuestionCard key={q.question} q={q} keys={false} answer={(choice) => answer(i, q, choice)} />)}
                  </li>
                )
              );
            })}
          </ul>
        </section>
      ))}
    </>
  );
}
