import { useEffect, useRef, useState } from "react";
import { inShell, listen, openInBrowser, request } from "./service.ts";
import { needsProvider, type Provider } from "./firstRun.ts";
import { modelChoices } from "./models.ts";
import type { Bot } from "./types.ts";

const EFFORTS = ["minimal", "low", "medium", "high"];

/**
 * The bot's model and thinking effort, a chip in the composer with a popover; a pick applies from the
 * next model call. Without a way to reach a model the chip is the sign-in. `report` shows an error
 * the popover is not open for.
 */
export function Model({ bot, bots, report }: { bot: Bot; bots: Bot[]; report: (message: string) => void }) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [serverModels, setServerModels] = useState<string[]>([]);
  const [error, setError] = useState("");
  /** Why the model list could not be read. */
  const [listError, setListError] = useState("");
  const [onPlan, setOnPlan] = useState(false);
  const [provider, setProvider] = useState<Provider>();
  const wrap = useRef<HTMLDivElement>(null);
  const chip = useRef<HTMLButtonElement>(null);
  const loadModels = () =>
    void request<{ models: string[] }>("list_models").then(
      (d) => { setServerModels(d.models); setListError(""); },
      (e: Error) => setListError(e.message),
    );
  // Again when the sign-in changes: the list is the plan's then, or the model server's.
  useEffect(() => {
    loadModels();
    return listen((frame) => {
      if (frame.type === "chatgpt_changed") loadModels();
    });
  }, []);
  // Only Bate's own window may read the sign-in, as in Settings.
  useEffect(() => {
    if (!inShell) return;
    const load = () => {
      request<{ signedIn: boolean }>("get_chatgpt").then((c) => setOnPlan(c.signedIn), () => {});
      request<Provider>("get_provider").then(setProvider, () => {});
    };
    load();
    return listen((frame) => {
      if (frame.type === "chatgpt_changed") load();
    });
  }, []);
  // While open: a click outside, or Esc, closes it. Esc is taken here so that it does not also stop the bot.
  useEffect(() => {
    if (!open) return;
    const away = (e: PointerEvent) => {
      if (!wrap.current?.contains(e.target as Node)) setOpen(false);
    };
    const key = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      setOpen(false);
      chip.current?.focus();
    };
    document.addEventListener("pointerdown", away);
    document.addEventListener("keydown", key);
    return () => {
      document.removeEventListener("pointerdown", away);
      document.removeEventListener("keydown", key);
    };
  }, [open]);

  const model = bot.model ?? "";
  const effort = bot.effort ?? "";
  const { models, typed } = modelChoices(serverModels, bots.flatMap((b) => (b.model ? [b.model] : [])), model, query);
  // An empty model: back to the engine's default, effort with it.
  const save = (m: string, e: string, done?: () => void) =>
    request("set_model", { bot: bot.id, model: m, ...(m && e && { effort: e }) }).then(
      () => { setError(""); done?.(); },
      (err: Error) => setError(err.message),
    );
  const pick = (m: string) => save(m, effort, () => setOpen(false));

  if (inShell && provider && needsProvider(onPlan, provider) && !model) {
    return <button type="button" className="small chip" data-tip="Sign in with ChatGPT to give this bot a model" onClick={() => void request<{ url: string }>("sign_in_chatgpt").then((d) => openInBrowser(d.url), (e: Error) => report(e.message))}>Sign in</button>;
  }
  const row = (m: string, label = m) => (
    <li key={m}>
      <button type="button" role="option" aria-selected={m === model} className="row plain" onClick={() => pick(m)}>
        <span className="label"><span>{label}</span></span>
        {m === model && <span aria-hidden="true">✓</span>}
      </button>
    </li>
  );
  const tip = model
    ? `This bot uses ${model}${effort ? `, thinking ${effort}` : ""}. Other bots keep their own.`
    : "App default: this bot has no model of its own";
  return (
    <div className="menu" ref={wrap}>
      <button
        ref={chip}
        type="button"
        className="small chip"
        data-tip={tip}
        aria-label={`Model: ${model || "app default"}. ${model ? "This bot" : "App default"}`}
        aria-haspopup="dialog"
        aria-expanded={open}
        onClick={() => {
          setQuery("");
          setOpen(!open);
        }}
      >
        <span>{model || "App default"}</span> <span aria-hidden="true">▾</span>
      </button>
      {open && (
        <div className="popover" role="dialog" aria-label="Model and thinking for this bot">
          <input
            autoFocus
            type="search"
            aria-label="Search models"
            placeholder={serverModels.length ? "Search models" : "Search, or type a model's name"}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key !== "Enter" || e.nativeEvent.isComposing) return;
              e.preventDefault();
              const first = typed ?? models[0];
              if (first) pick(first);
            }}
          />
          <ul role="listbox" aria-label="Models">
            {!query.trim() && row("", "App default")}
            {models.map((m) => row(m))}
            {typed && row(typed, `Use “${typed}”`)}
          </ul>
          {query.trim() && !models.length && !typed && <p className="note">No model matches.</p>}
          <label>
            Thinking{" "}
            <select value={effort} disabled={!model} aria-describedby={model ? undefined : "effort-why"} onChange={(e) => void save(model, e.target.value)}>
              <option value="">default</option>
              {EFFORTS.map((e) => <option key={e}>{e}</option>)}
            </select>
          </label>
          {!model && <small id="effort-why">Choose a model first: the thinking effort is set with it.</small>}
          {error && <span className="error" role="alert">{error}</span>}
          {listError && (
            <span className="error" role="alert">
              The model list could not be read: {listError}{" "}
              <button type="button" onClick={loadModels}>Try again</button>
            </span>
          )}
          {onPlan && (
            <small>
              Using ChatGPT plan · <a href="https://chatgpt.com/settings/usage" target="_blank" rel="noreferrer">Manage usage</a>
            </small>
          )}
        </div>
      )}
    </div>
  );
}
