import { useEffect, useRef, useState } from "react";
import { listen, request } from "./service.ts";
import { add, type Entry, type Item, type Question } from "./chat.ts";
import type { Bot } from "./types.ts";

type Opened = { history: { entry: Entry }[]; questions: Question[] };

export function Chat({ bot }: { bot: Bot }) {
  const [items, setItems] = useState<Item[]>([]);
  const [questions, setQuestions] = useState<Question[]>([]);
  const [text, setText] = useState("");
  const [error, setError] = useState("");
  const [sending, setSending] = useState(false);
  // State is only read again at the next render; a second click in the same moment must see it now.
  const inFlight = useRef(false);

  useEffect(() => {
    let gone = false;
    const open = () =>
      request<Opened>("open_bot", { bot: bot.id }).then((d) => {
        if (gone) return;
        setItems(d.history.map((h) => h.entry).reduce(add, []));
        setQuestions(d.questions);
      }, (e) => setError(e.message));
    open();
    const stop = listen((frame) => {
      if (frame.bot !== bot.id) return;
      if (frame.type === "transcript") {
        const entry = frame.entry as Entry;
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
      <ol className="items">
        {items.map((item, i) => (
          <li key={i} className={item.kind}>
            {item.kind === "tool" ? (
              <details>
                <summary>
                  {item.tool} {item.output === undefined ? "…" : item.isError ? "failed" : "done"}
                </summary>
                <pre>{JSON.stringify(item.args, null, 2)}</pre>
                {item.output !== undefined && <pre>{item.output}</pre>}
              </details>
            ) : item.kind === "ended" ? (
              <p>{item.reason === "aborted" ? "Stopped." : `The reply failed: ${item.error ?? item.reason}`}</p>
            ) : (
              <p>{item.text}</p>
            )}
          </li>
        ))}
      </ol>
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
