import { useEffect, useId, useState } from "react";
import { request } from "./service.ts";
import type { Bot } from "./types.ts";

const EFFORTS = ["minimal", "low", "medium", "high"];

/** The bot's model and thinking effort; a change applies from the next model call. */
export function Model({ bot, bots }: { bot: Bot; bots: Bot[] }) {
  const [model, setModel] = useState(bot.model ?? "");
  const [effort, setEffort] = useState(bot.effort ?? "");
  const [error, setError] = useState("");
  const list = useId();
  useEffect(() => {
    setModel(bot.model ?? "");
    setEffort(bot.effort ?? "");
  }, [bot.model, bot.effort]);
  // shortcut: offers the models other bots use; list the server's models once #8 moves its address into the service.
  const known = [...new Set(bots.flatMap((b) => (b.model ? [b.model] : [])))];
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
    </form>
  );
}
