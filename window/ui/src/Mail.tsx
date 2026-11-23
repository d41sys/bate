import { useEffect, useState } from "react";
import { when } from "./conversations.ts";
import { Revealed } from "./Reveal.tsx";
import { listen, request } from "./service.ts";
import type { Bot } from "./types.ts";

/** The acceptance command of a hand-off: `state` is none until the worker has done. `note`: how it failed, or why it was not run. */
type Check = { command: string; state: "asking" | "running" | "retry" | "passed" | "failed" | "not_run" | null; note: string; output: string };
type Mail = { seq: number; from: string; to: string; text: string; state: "waiting" | "working" | "answered"; answer?: string; error?: string; atMs: number; check?: Check };
const STATE = { waiting: "Waiting", working: "In progress", answered: "Answered" };
/** Answered with a reason instead of an answer: the other bot failed, or the message was cancelled. */
const failed = (m: Mail) => m.state === "answered" && !!m.error;
const CHECK = { asking: "Waiting for you", running: "Running", retry: "Failed, the bot tries again", passed: "Passed", failed: "Failed", not_run: "Not run" };

/** The command that tests the work, shown in both bots' lists: its state in the summary, the whole command and what it printed once opened. */
function CheckLine({ c }: { c: Check }) {
  const state = c.state ? CHECK[c.state] : "Not run yet";
  const line = c.command.split("\n")[0];
  return (
    <details className="check">
      <summary className={c.state === "failed" ? "error" : undefined}>Check: <code>{line.length > 60 ? `${line.slice(0, 59)}…` : line}</code> · {state}</summary>
      <pre><Revealed text={c.command} /></pre>
      {c.note && <small>{c.note}</small>}
      {c.output.trim() && <pre>{c.output}</pre>}
    </details>
  );
}

/** Messages between this bot and other bots: what waits, what is in progress, what was answered or got no answer. */
export function Mail({ bot, bots }: { bot: string; bots: Bot[] }) {
  const [mail, setMail] = useState<Mail[]>([]);
  // `seq` is the message whose Cancel failed; none, for the list itself.
  const [error, setError] = useState<{ seq?: number; text: string }>();

  useEffect(() => {
    const load = () => request<{ mail: Mail[] }>("list_mail", { bot }).then((d) => setMail(d.mail), (e: Error) => setError({ text: e.message }));
    load();
    return listen((frame) => {
      if (frame.type === "mail_changed") load();
    });
  }, [bot]);

  // No messages, no line: most bots never write to another.
  if (!mail.length && !error) return null;
  const name = (id: string) => bots.find((b) => b.id === id)?.name ?? "(deleted bot)";
  const cancel = (seq: number) => request("cancel_mail", { seq }).then(() => setError(undefined), (e: Error) => setError({ seq, text: e.message }));
  const open = mail.filter((m) => m.state !== "answered").length;

  return (
    <details className="banner mail">
      <summary>Messages with other bots · {open ? `${open} open` : "none open"}</summary>
      <ul className="rows">
        {mail.map((m) => {
          const title = m.from === bot ? `To ${name(m.to)}` : `From ${name(m.from)}`;
          return (
            <li key={m.seq}>
              <div className="what">
                <strong>{title}</strong>
                <span className="note"> · {when(m.atMs)}</span>
                <div>{m.text.slice(0, 200)}</div>
                {m.check && <CheckLine c={m.check} />}
                {m.state === "answered" && (failed(m) ? <small className="error">No answer: {m.error}</small> : <small>Answer: {(m.answer ?? "").slice(0, 200)}</small>)}
                {error?.seq === m.seq && <small className="error" role="alert">{error.text}</small>}
              </div>
              <div className="actions">
                <span className={failed(m) ? "badge" : "badge quiet"}>{failed(m) ? "No answer" : STATE[m.state]}</span>
                {m.state === "waiting" && <button type="button" className="small" aria-label={`Cancel: ${title}`} onClick={() => cancel(m.seq)}>Cancel</button>}
              </div>
            </li>
          );
        })}
      </ul>
      {error && error.seq === undefined && <p className="error" role="alert">{error.text}</p>}
    </details>
  );
}
