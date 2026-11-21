import { useEffect, useState } from "react";
import { RowMenu } from "./RowMenu.tsx";
import { Sheet } from "./Sheet.tsx";
import { dayTime, EVERY, when } from "./times.ts";
import { heldText, skipWhy } from "./routines.ts";
import { listen, openInBrowser, request } from "./service.ts";
import type { Frequency, Routine, RoutineRun } from "./types.ts";

const OUTCOME: Record<RoutineRun["outcome"], string> = { sent: "Sent", failed: "Failed", missed: "Missed", limit: "Stopped", skipped: "Skipped" };

/** The fields of the dialog; a routine with no schedule starts only from its web address or a chat. */
const blank = { name: "", prompt: "", scheduled: true, frequency: "daily" as Frequency, interval: 1, time: "09:00", webhook: false, chat: false, precheck: "" };
const formOf = (r: Routine): typeof blank => ({ name: r.name, prompt: r.prompt, scheduled: !!r.schedule, frequency: r.schedule?.frequency ?? blank.frequency, interval: r.schedule?.interval ?? 1, time: r.schedule?.time ?? blank.time, webhook: r.webhook, chat: r.chat, precheck: r.precheck ?? "" });

/** How a run ended: a quiet badge when it went well, a plain one when the person should look. */
const Outcome = ({ run }: { run: RoutineRun }) => <span className={run.outcome === "sent" || run.outcome === "skipped" ? "badge quiet" : "badge"}>{OUTCOME[run.outcome]}</span>;

function PastRuns({ bot, routineId }: { bot: string; routineId: string }) {
  const [open, setOpen] = useState(false);
  const [runs, setRuns] = useState<RoutineRun[]>();
  const [error, setError] = useState("");

  useEffect(() => {
    if (!open) return;
    const load = () =>
      request<{ runs: RoutineRun[] }>("list_routine_runs", { bot, routineId }).then((d) => (setRuns(d.runs), setError("")), (e: Error) => setError(e.message));
    load();
    return listen((frame) => {
      if (frame.type === "routines_changed" && frame.bot === bot) load();
    });
  }, [open, bot, routineId]);

  return (
    <details onToggle={(e) => setOpen(e.currentTarget.open)}>
      <summary>Past runs</summary>
      {error && <p className="error" role="alert">{error}</p>}
      {!runs && !error && <p className="note">Loading runs…</p>}
      {runs && (runs.length === 0 ? (
        <p className="note">No runs yet.</p>
      ) : (
        <ul className="rows">
          {runs.map((x, i) => (
            <li key={`${x.atMs}-${i}`}>
              <span>{dayTime(x.atMs)}</span>
              {x.error && <small className="error">{x.error}</small>}
              {skipWhy(x) && <small>{skipWhy(x)}</small>}
              {x.by === "user" && <small>Started by you</small>}
              <Outcome run={x} />
            </li>
          ))}
        </ul>
      ))}
    </details>
  );
}

/** A bot's routines: one row each (when it runs, how the last run ended, on or off), and New and Edit in a dialog. */
export function Routines({ bot }: { bot: string }) {
  const [routines, setRoutines] = useState<Routine[]>();
  /** The routine being edited, or `{}` for a new one. */
  const [ask, setAsk] = useState<{ edit?: Routine }>();
  const [form, setForm] = useState(blank);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    const load = () =>
      request<{ routines: Routine[] }>("list_routines", { bot }).then((d) => (setRoutines(d.routines), setError("")), (e: Error) => setError(e.message));
    load();
    return listen((frame) => {
      if (frame.type === "routines_changed" && frame.bot === bot) load();
    });
  }, [bot]);

  /** Whether it worked; if not, the error shows. */
  const run = (p: Promise<unknown>, done = false) =>
    p.then(() => (setError(""), setSaved(done), true), (e: Error) => (setError(e.message), setSaved(false), false));
  const open = (edit?: Routine) => (setForm(edit ? formOf(edit) : blank), setAsk({ edit }));
  const save = async () => {
    const { edit } = ask!;
    const { name, prompt, scheduled, frequency, interval, time, webhook, chat, precheck } = form;
    if (!scheduled && !webhook && !chat) throw new Error("Choose what starts it: a schedule, a web address or a chat.");
    await request("set_routine", {
      bot,
      routine: { ...edit, name, prompt, enabled: edit?.enabled ?? true, webhook, chat, precheck: precheck.trim() || undefined, schedule: scheduled ? { ...edit?.schedule, frequency, interval, time } : undefined },
    });
    setError("");
    setSaved(true);
  };

  return (
    <section>
      {!routines && !error && <p className="note">Loading routines…</p>}
      {routines?.length === 0 && (
        <div className="empty">
          <p>A routine is a prompt the bot gets by itself: on a schedule, from a web address or from a chat.</p>
          <button onClick={() => open()}>New routine</button>
        </div>
      )}
      {!!routines?.length && (
        <>
          <ul className="rows">
            {routines.map((r) => (
              <li key={r.id}>
                <div className="what">
                  <strong>{r.name}</strong> {r.lastRun && <Outcome run={r.lastRun} />}
                  {r.backoff && <span className="badge quiet">{heldText(r.backoff)}</span>}
                  <small>{when(r)}</small>
                  <small>
                    {r.enabled && (r.backoff?.nextRunMs ?? r.nextRunMs) !== undefined && `Next: ${dayTime((r.backoff?.nextRunMs ?? r.nextRunMs)!)}`}
                    {r.enabled && (r.backoff?.nextRunMs ?? r.nextRunMs) !== undefined && r.lastRun && " · "}
                    {r.lastRun && `Last: ${dayTime(r.lastRun.atMs)}${r.lastRun.by === "user" ? " (started by you)" : ""}`}
                  </small>
                  {r.lastRun?.error && <small className="error">{r.lastRun.error}</small>}
                  {r.lastRun && skipWhy(r.lastRun) && <small>{skipWhy(r.lastRun)}</small>}
                  {r.lastRun?.reason === "signed_out" && r.lastRun.detail && <button className="small" onClick={() => void run(request<{ url: string }>("sign_in_connector", { bot, name: r.lastRun!.detail }).then((d) => openInBrowser(d.url)))}>Sign in to {r.lastRun.detail}</button>}
                  {r.precheck && <small>Checks first: <code>{r.precheck}</code></small>}
                  {r.chat && <small>From a paired chat: write /run {r.name}</small>}
                  {r.hookUrl && <small>Web address: <code>{r.hookUrl}</code></small>}
                </div>
                <div className="actions">
                  <button className="small" onClick={() => void run(request("run_routine", { bot, routineId: r.id }))}>Test</button>
                  <label>
                    <input type="checkbox" role="switch" aria-label={r.name} checked={r.enabled} onChange={(e) => void run(request("set_routine", { bot, routine: { ...r, enabled: e.target.checked } }), true)} />
                    {r.enabled ? "On" : "Off"}
                  </label>
                  <RowMenu
                    label={r.name}
                    items={[
                      { label: "Edit…", run: () => open(r) },
                      { label: "Delete", danger: true, run: () => void run(request("delete_routine", { bot, routineId: r.id })) },
                    ]}
                  />
                </div>
                {r.id && <PastRuns bot={bot} routineId={r.id} />}
              </li>
            ))}
          </ul>
          <div className="actions">
            <button onClick={() => open()}>New routine…</button>
            {saved && <span className="note" role="status">Saved</span>}
          </div>
        </>
      )}
      {error && <p className="error" role="alert">{error}</p>}
      {ask && (
        <Sheet title={ask.edit ? "Edit routine" : "New routine"} action={ask.edit ? "Save" : "Add"} go={save} close={() => setAsk(undefined)}>
          <label>Name <input required maxLength={80} value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} /></label>
          <label>Prompt <textarea required rows={4} maxLength={8000} value={form.prompt} onChange={(e) => setForm({ ...form, prompt: e.target.value })} /></label>
          <>
            <h3>What starts it</h3>
            <label><input type="checkbox" checked={form.scheduled} onChange={(e) => setForm({ ...form, scheduled: e.target.checked })} /> A schedule</label>
            {form.scheduled && (
              <>
                <div className="field">
                  <label htmlFor="routine-every">Every</label>
                  <div className="find">
                    <input id="routine-every" type="number" min={1} max={1440} value={form.interval} onChange={(e) => setForm({ ...form, interval: Number(e.target.value) })} />
                    <select aria-label="Unit" value={form.frequency} onChange={(e) => setForm({ ...form, frequency: e.target.value as Frequency })}>
                      {(Object.keys(EVERY) as Frequency[]).map((f) => <option key={f} value={f}>{EVERY[f]}</option>)}
                    </select>
                  </div>
                </div>
                <label>From <input type="time" required value={form.time} onChange={(e) => setForm({ ...form, time: e.target.value })} /></label>
              </>
            )}
            <label><input type="checkbox" checked={form.webhook} onChange={(e) => setForm({ ...form, webhook: e.target.checked })} /> A web address</label>
            {form.webhook && <p className="note">The address is shown under the routine. Anyone on this Mac who knows it can start the routine. For a new address, turn this off and save, then turn it on and save.</p>}
            <label><input type="checkbox" checked={form.chat} onChange={(e) => setForm({ ...form, chat: e.target.checked })} /> A paired chat (Telegram, Discord), by writing /run and its name</label>
            <label>Check first, with a command <input maxLength={500} value={form.precheck} onChange={(e) => setForm({ ...form, precheck: e.target.value })} /></label>
            {form.precheck.trim() && <p className="note">Runs before each run, without a model, in the bot's folder and under its limits. If it ends with 0, the bot runs and gets what it printed; any other end skips the run. Saving allows exactly this command for this routine, without asking again.</p>}
          </>
        </Sheet>
      )}
    </section>
  );
}
