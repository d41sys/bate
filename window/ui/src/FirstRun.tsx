import { useEffect, useRef, useState } from "react";
import { inShell, listen, openInBrowser, request } from "./service.ts";
import { FIRST_BOT, needsProvider, type Provider } from "./firstRun.ts";
import type { Bot } from "./types.ts";

/**
 * Whether no model can be reached yet: neither ChatGPT nor a model server. `undefined` until the
 * service has said, and for good on another device, which may not read the sign-in. `refresh`
 * asks again (saving a model server sends no frame); `reload` does too, when it changes.
 */
export function useNeedsProvider(reload: string): [boolean | undefined, () => void] {
  const [needed, setNeeded] = useState<boolean>();
  const [again, setAgain] = useState(0);
  useEffect(() => {
    if (!inShell) return;
    const load = () =>
      Promise.all([request<{ signedIn: boolean }>("get_chatgpt"), request<Provider>("get_provider")]).then(
        ([chatgpt, provider]) => setNeeded(needsProvider(chatgpt.signedIn, provider)),
        () => {},
      );
    void load();
    return listen((frame) => frame.type === "chatgpt_changed" && void load());
  }, [reload, again]);
  return [needed, () => setAgain((n) => n + 1)];
}

/**
 * The first screen of a new install: the sign-in, and "Set up later". It stays mounted while the
 * person signs in, so it sees `needed` end and makes the first bot (when there is none), then
 * says which to open.
 */
export function FirstRun({ needed, hasBots, refresh, later, opened }: {
  needed: boolean | undefined;
  hasBots: boolean;
  refresh: () => void;
  later: () => void;
  opened: (bot: string, message: string) => void;
}) {
  const [url, setUrl] = useState("");
  const [key, setKey] = useState("");
  const [model, setModel] = useState("");
  const [error, setError] = useState("");
  const wasNeeded = useRef(false);
  // The model of a model server, kept until the bot it is for exists.
  const chosen = useRef("");
  useEffect(() => listen((frame) => typeof frame.error === "string" && frame.type === "chatgpt_changed" && setError(frame.error)), []);
  useEffect(() => {
    const signedIn = wasNeeded.current && needed === false;
    wasNeeded.current = needed === true;
    if (!signedIn || hasBots) return;
    void (async () => {
      const { bot } = await request<{ bot: Bot }>("create_bot", { name: FIRST_BOT.name });
      await request("set_bot", { bot: bot.id, instructions: FIRST_BOT.purpose, avatar: FIRST_BOT.avatar });
      if (chosen.current) await request("set_model", { bot: bot.id, model: chosen.current });
      opened(bot.id, FIRST_BOT.message);
    })().catch((e: Error) => setError(e.message));
  }, [needed]);
  // Signed in: the screen is gone, but a failure while making the bot is still said.
  if (!needed) return error ? <p className="banner error" role="alert" onClick={() => setError("")}>{error}</p> : null;
  const run = (p: Promise<unknown>) => p.then(() => setError(""), (e: Error) => setError(e.message));
  return (
    <section className="first-run" aria-label="First run">
      <h1>Welcome to Bate</h1>
      <p>Sign in, and Bate makes your first bot to talk to. The sign-in happens in your browser.</p>
      <button className="primary" onClick={() => run(request<{ url: string }>("sign_in_chatgpt").then((d) => openInBrowser(d.url)))}>Sign in with ChatGPT</button>
      <details>
        <summary>Use a model server instead</summary>
        <form
          className="fields stack"
          onSubmit={(e) => {
            e.preventDefault();
            chosen.current = model.trim();
            run(request("set_provider", { url: url.trim(), key: key.trim() }).then(refresh));
          }}
        >
          <label>URL <input required placeholder="http://127.0.0.1:1234/v1" value={url} onChange={(e) => setUrl(e.target.value)} /></label>
          <label>Model <input required placeholder="the model's name" value={model} onChange={(e) => setModel(e.target.value)} /></label>
          <label>API key <input type="password" placeholder="optional" value={key} onChange={(e) => setKey(e.target.value)} /></label>
          <button>Use this server</button>
        </form>
      </details>
      <button className="ghost" onClick={later}>Set up later</button>
      {error && <p className="error" role="alert">{error}</p>}
    </section>
  );
}
