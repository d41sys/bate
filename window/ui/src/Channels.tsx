import { useEffect, useState } from "react";
import { CodeBlock, Field, Problem, Saved, useSaves } from "./Form.tsx";
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
  const [loaded, setLoaded] = useState(false);
  const { run, fail, edit, saved, error } = useSaves();
  const load = () =>
    request<{ channels: Channel[] }>("get_channels", { bot }).then((r) => (setLoaded(true), setChannels(r.channels)), (e: Error) => fail("load", e.message));
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
  const act = (key: string, p: Promise<unknown>) => run(key, p).then(load);
  const left = code ? Math.max(0, Math.round((code.expiresAt - now) / 1000)) : 0;

  return (
    <>
      <h2>Channels</h2>
      <p>Write to this bot from a chat app. It answers when its turn ends; what it asks you is still answered here.</p>
      {!loaded && !error("load") && <p className="note">Loading…</p>}
      <Problem text={error("load")} />
      {channels.map((c) => (
        <form
          key={c.kind}
          className="fields stack"
          onSubmit={(e) => {
            e.preventDefault();
            act(`token-${c.kind}`, request("set_channel", { bot, kind: c.kind, token: tokens[c.kind] ?? "" }).then(() => setTokens({ ...tokens, [c.kind]: "" })));
          }}
        >
          <Field id={`token-${c.kind}`} label={`${NAMES[c.kind] ?? c.kind} bot token`} hint={c.hasToken ? "A saved token is never shown again; enter a new one to change it." : undefined} error={error(`token-${c.kind}`)}>
            <input type="password" autoComplete="off" value={tokens[c.kind] ?? ""} placeholder={c.hasToken ? "saved" : ""} onChange={(e) => (edit(`token-${c.kind}`), setTokens({ ...tokens, [c.kind]: e.target.value }))} />
          </Field>
          <div className="actions">
            <button type="submit" disabled={!tokens[c.kind]?.trim()}>Save</button>
            {c.hasToken && (
              <button type="button" aria-label={`Remove the ${NAMES[c.kind]} token`} onClick={() => act(`token-${c.kind}`, request("set_channel", { bot, kind: c.kind, token: "" }))}>Remove</button>
            )}
            {c.hasToken && !(code?.kind === c.kind && c.pairing && left > 0) && (
              <button
                type="button"
                onClick={() =>
                  run(`pair-${c.kind}`, request<Omit<Code, "kind">>("pair_channel", { bot, kind: c.kind }).then((r) => (setCode({ ...r, kind: c.kind }), setNow(Date.now())))).then(load)
                }
              >
                Pair a chat
              </button>
            )}
            <Saved on={saved(`token-${c.kind}`)} />
          </div>
          <Problem text={error(`pair-${c.kind}`)} />
          {c.hasToken && code?.kind === c.kind && c.pairing && left > 0 && (
            <>
              <p>Send this code to the bot in {NAMES[c.kind]} ({Math.floor(left / 60)}:{String(left % 60).padStart(2, "0")} left):</p>
              <CodeBlock key={code.code} text={code.code} label="the pairing code" />
            </>
          )}
          <Problem text={c.error ? `${NAMES[c.kind]} is not connected: ${c.error}` : undefined} />
          {c.hasToken && !c.chats.length && <p className="note">No chat is paired yet. Choose Pair a chat, then send the code to the bot.</p>}
          {!!c.chats.length && (
            <ul className="rows">
              {c.chats.map((chat) => (
                <li key={chat.chat}>
                  <div className="what"><strong>{chat.name}</strong></div>
                  <div className="actions">
                    <button type="button" className="small" aria-label={`Remove ${chat.name}`} onClick={() => act(`chat-${c.kind}`, request("remove_chat", { bot, kind: c.kind, chat: chat.chat }))}>Remove</button>
                  </div>
                </li>
              ))}
            </ul>
          )}
          <Problem text={error(`chat-${c.kind}`)} />
        </form>
      ))}
    </>
  );
}
