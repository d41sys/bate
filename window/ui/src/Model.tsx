import { useEffect, useId, useState } from "react";
import { inShell, listen, openInBrowser, request } from "./service.ts";
import type { Bot } from "./types.ts";

const EFFORTS = ["minimal", "low", "medium", "high"];
const USAGE = "https://chatgpt.com/settings/usage";

/** The bot's model and thinking effort; a change applies from the next model call. */
export function Model({ bot, bots }: { bot: Bot; bots: Bot[] }) {
  const [model, setModel] = useState(bot.model ?? "");
  const [effort, setEffort] = useState(bot.effort ?? "");
  const [serverModels, setServerModels] = useState<string[]>([]);
  const [error, setError] = useState("");
  const list = useId();
  useEffect(() => {
    setModel(bot.model ?? "");
    setEffort(bot.effort ?? "");
  }, [bot.model, bot.effort]);
  useEffect(() => {
    void request<{ models: string[] }>("list_models").then(
      (d) => setServerModels(d.models),
      () => {},
    );
  }, []);
  const [onPlan, setOnPlan] = useState(false);
  // Only Bate's own window may read the sign-in, as in Settings.
  useEffect(() => {
    if (!inShell) return;
    const load = () => request<{ signedIn: boolean }>("get_chatgpt").then((c) => setOnPlan(c.signedIn), () => {});
    load();
    return listen((frame) => {
      if (frame.type === "chatgpt_changed") load();
    });
  }, []);
  const known = [...new Set([...serverModels, ...bots.flatMap((b) => (b.model ? [b.model] : []))])];
  // An empty model: back to the engine's default, effort with it.
  const save = (m: string, e: string) => {
    request("set_model", { bot: bot.id, model: m.trim(), ...(m.trim() && e && { effort: e }) }).then(
      () => setError(""),
      (err: Error) => setError(err.message),
    );
  };
  return (
    <form className="model" onSubmit={(e) => { e.preventDefault(); save(model, effort); }}>
      <label>
        Model <input list={list} placeholder="the engine's default" value={model} onChange={(e) => setModel(e.target.value)} />
      </label>
      <datalist id={list}>{known.map((m) => <option key={m} value={m} />)}</datalist>
      <label>
        Thinking{" "}
        <select value={effort} disabled={!model.trim()} onChange={(e) => setEffort(e.target.value)}>
          <option value="">default</option>
          {EFFORTS.map((e) => <option key={e}>{e}</option>)}
        </select>
      </label>
      <button disabled={model.trim() === (bot.model ?? "") && effort === (bot.effort ?? "")}>Use</button>
      {error && <span className="error" role="alert">{error}</span>}
      {onPlan && (
        <span>
          Using ChatGPT plan ·{" "}
          <a href={USAGE} onClick={(e) => { e.preventDefault(); openInBrowser(USAGE); }}>Manage usage</a>
        </span>
      )}
    </form>
  );
}
