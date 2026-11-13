import { useEffect, useState } from "react";
import { listen, request } from "./service.ts";
import type { Frequency, Routine } from "./types.ts";

const EVERY: Record<Frequency, string> = {
  minutes: "minutes",
  hourly: "hours",
  daily: "days",
  weekly: "weeks",
  monthly: "months",
  yearly: "years",
};

const blank = { name: "", prompt: "", frequency: "daily" as Frequency, interval: 1, time: "09:00", webhook: false };

export function Routines({ bot }: { bot: string }) {
  const [routines, setRoutines] = useState<Routine[]>([]);
  const [form, setForm] = useState(blank);
  const [error, setError] = useState("");

  useEffect(() => {
    const load = () =>
      request<{ routines: Routine[] }>("list_routines", { bot }).then((d) => setRoutines(d.routines), (e) => setError(e.message));
    load();
    return listen((frame) => {
      if (frame.type === "routines_changed" && frame.bot === bot) load();
    });
  }, [bot]);

  const run = (p: Promise<unknown>) => p.then(() => setError(""), (e: Error) => setError(e.message));
  const save = (routine: Routine) => run(request("set_routine", { bot, routine }));
  const create = async () => {
    const { frequency, interval, time } = form;
    await save({ name: form.name, prompt: form.prompt, enabled: true, webhook: form.webhook, schedule: { frequency, interval, time } });
    setForm(blank);
  };

  return (
    <section>
      <ul className="rows">
        {routines.map((r) => (
          <li key={r.id}>
            <label>
              <input type="checkbox" checked={r.enabled} onChange={(e) => save({ ...r, enabled: e.target.checked })} /> {r.name}
            </label>
            <span className="note">
              {r.schedule && `every ${r.schedule.interval} ${EVERY[r.schedule.frequency]} from ${r.schedule.time}`}
              {r.nextRunMs !== undefined && ` · next ${new Date(r.nextRunMs).toLocaleString()}`}
              {r.lastRun && ` · last ${r.lastRun.outcome}${r.lastRun.error ? `: ${r.lastRun.error}` : ""}`}
            </span>
            {r.hookUrl && <code>{r.hookUrl}</code>}
            <button onClick={() => run(request("delete_routine", { bot, routineId: r.id }))}>Delete</button>
          </li>
        ))}
      </ul>
      <form className="fields" onSubmit={(e) => { e.preventDefault(); create(); }}>
        <h2>New routine</h2>
        <label>Name <input required maxLength={80} value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} /></label>
        <label>Prompt <textarea required maxLength={8000} value={form.prompt} onChange={(e) => setForm({ ...form, prompt: e.target.value })} /></label>
        <label>
          Every{" "}
          <input type="number" min={1} max={1440} value={form.interval} onChange={(e) => setForm({ ...form, interval: Number(e.target.value) })} />{" "}
          <select value={form.frequency} onChange={(e) => setForm({ ...form, frequency: e.target.value as Frequency })}>
            {(Object.keys(EVERY) as Frequency[]).map((f) => <option key={f} value={f}>{EVERY[f]}</option>)}
          </select>
        </label>
        <label>From <input type="time" required value={form.time} onChange={(e) => setForm({ ...form, time: e.target.value })} /></label>
        <label><input type="checkbox" checked={form.webhook} onChange={(e) => setForm({ ...form, webhook: e.target.checked })} /> Also run from a web address</label>
        <button>Add</button>
      </form>
      {error && <p className="error" role="alert">{error}</p>}
    </section>
  );
}
