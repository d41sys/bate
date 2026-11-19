import { useEffect, useState } from "react";
import { listen, request } from "./service.ts";
import type { Bot } from "./types.ts";

type Mail = { seq: number; from: string; to: string; text: string; state: "waiting" | "working" | "answered"; answer?: string; error?: string; atMs: number };

const STATE = { waiting: "waiting", working: "in progress", answered: "answered" };

/** Messages between this bot and other bots: what waits, what is in progress, what was answered. */
export function Mail({ bot, bots }: { bot: string; bots: Bot[] }) {
  const [mail, setMail] = useState<Mail[]>([]);
  const [error, setError] = useState("");

  useEffect(() => {
    const load = () => request<{ mail: Mail[] }>("list_mail", { bot }).then((d) => setMail(d.mail), (e) => setError(e.message));
    load();
    return listen((frame) => {
      if (frame.type === "mail_changed") load();
    });
  }, [bot]);

  if (!mail.length && !error) return null;
  const name = (id: string) => bots.find((b) => b.id === id)?.name ?? "(deleted bot)";
  const cancel = (seq: number) => request("cancel_mail", { seq }).then(() => setError(""), (e: Error) => setError(e.message));

  return (
    <details className="mail">
      <summary>Messages with other bots ({mail.filter((m) => m.state !== "answered").length} open)</summary>
      <ol>
        {mail.map((m) => (
          <li key={m.seq}>
            {m.from === bot ? `To ${name(m.to)}` : `From ${name(m.from)}`} · {STATE[m.state]}: {m.text.slice(0, 200)}
            {m.state === "answered" && <span className="note"> → {m.error ? `no answer: ${m.error}` : (m.answer ?? "").slice(0, 200)}</span>}
            {m.state === "waiting" && <button type="button" onClick={() => cancel(m.seq)}>Cancel</button>}
          </li>
        ))}
      </ol>
      {error && <p className="error" role="alert">{error}</p>}
    </details>
  );
}
