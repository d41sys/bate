import { useEffect, useState } from "react";
import { listen, request } from "./service.ts";

type Channel = { kind: string; hasToken: boolean; pairing: boolean; chats: { chat: string; name: string }[]; error: string | null };
type Code = { kind: string; code: string; expiresAt: number };

const NAMES: Record<string, string> = { telegram: "Telegram", discord: "Discord" };

/** Writing to the bot from Telegram or Discord: each one's bot token, who may write, and letting one more in. */
export function Channels({ bot }: { bot: string }) {
  const [channels, setChannels] = useState<Channel[]>([]);
  const [tokens, setTokens] = useState<Record<string, string>>({});
  const [code, setCode] = useState<Code>();
  const [now, setNow] = useState(Date.now());
  const [error, setError] = useState("");
  const load = () =>
    request<{ channels: Channel[] }>("get_channels", { bot }).then((r) => (setError(""), setChannels(r.channels)), (e: Error) => setError(e.message));
  useEffect(() => {
    void load();
    // Chats pair from their side, and a channel connects or fails on its own: the service says when.
    const stop = listen((frame) => {
      if (frame.type === "channels_changed" && frame.bot === bot) void load();
    });
    // Counts the code down.
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => (stop(), clearInterval(timer));
  }, [bot]);
  const run = (p: Promise<unknown>) => p.then(() => (setError(""), load()), (e: Error) => setError(e.message));
  const left = code ? Math.max(0, Math.round((code.expiresAt - now) / 1000)) : 0;

  return (
    <>
      <h2>Channels</h2>
      <p>Write to this bot from a chat app. It answers when its turn ends; what it asks you is still answered here.</p>
      {channels.map((c) => (
        <form
          key={c.kind}
          className="fields"
          onSubmit={(e) => {
            e.preventDefault();
            run(request("set_channel", { bot, kind: c.kind, token: tokens[c.kind] ?? "" }).then(() => setTokens({ ...tokens, [c.kind]: "" })));
          }}
        >
          <label>
            {NAMES[c.kind] ?? c.kind} bot token{" "}
            <input
              type="password"
              autoComplete="off"
              value={tokens[c.kind] ?? ""}
              placeholder={c.hasToken ? "saved" : ""}
              onChange={(e) => setTokens({ ...tokens, [c.kind]: e.target.value })}
            />
          </label>
          <p>
            <button type="submit" disabled={!tokens[c.kind]?.trim()}>
              Save
            </button>{" "}
            {c.hasToken && (
              <button type="button" onClick={() => run(request("set_channel", { bot, kind: c.kind, token: "" }))}>
                Remove
              </button>
            )}{" "}
            {c.hasToken &&
              (code?.kind === c.kind && c.pairing && left > 0 ? (
                <span>
                  Send <strong className="code">{code.code}</strong> to the bot in {NAMES[c.kind]} ({Math.floor(left / 60)}:
                  {String(left % 60).padStart(2, "0")} left)
                </span>
              ) : (
                <button
                  type="button"
                  onClick={() =>
                    request<Omit<Code, "kind">>("pair_channel", { bot, kind: c.kind }).then(
                      (r) => (setCode({ ...r, kind: c.kind }), setNow(Date.now()), load()),
                      (e: Error) => setError(e.message),
                    )
                  }
                >
                  Pair a chat
                </button>
              ))}
          </p>
          {c.error && <p className="error" role="alert">{NAMES[c.kind]} is not connected: {c.error}</p>}
          {!!c.chats.length && (
            <ul className="rows">
              {c.chats.map((chat) => (
                <li key={chat.chat}>
                  {chat.name}{" "}
                  <button type="button" onClick={() => run(request("remove_chat", { bot, kind: c.kind, chat: chat.chat }))}>
                    Remove
                  </button>
                </li>
              ))}
            </ul>
          )}
        </form>
      ))}
      {error && <p className="error" role="alert">{error}</p>}
    </>
  );
}
