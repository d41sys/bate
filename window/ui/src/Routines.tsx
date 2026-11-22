import { useEffect, useState } from "react";
import { RowMenu } from "./RowMenu.tsx";
import { Sheet } from "./Sheet.tsx";
import { dayTime, EVERY, when } from "./times.ts";
import { afterText, checkText, heldText, skipWhy, watchArguments, watchText, watchTools } from "./routines.ts";
import { tokenCount } from "./usage.ts";
import { inShell, listen, openInBrowser, request } from "./service.ts";
import type { After, Bot, Connector, Frequency, Permissions, Routine, RoutineRun } from "./types.ts";
import { useLater } from "./useLater.ts";

const OUTCOME: Record<RoutineRun["outcome"], string> = { sent: "Sent", failed: "Failed", missed: "Missed", limit: "Stopped", skipped: "Skipped" };

/** The fields of the dialog; a routine with no schedule starts only from its web address or a chat. */
const blank = { name: "", prompt: "", scheduled: true, frequency: "daily" as Frequency, interval: 1, time: "09:00", after: false, afterBot: "", afterOutcome: "done" as After["outcome"], webhook: false, chat: false, precheck: "", watching: false, connector: "", tool: "", args: "{}", every: 5 };
const formOf = (r: Routine): typeof blank => ({ name: r.name, prompt: r.prompt, scheduled: !!r.schedule, frequency: r.schedule?.frequency ?? blank.frequency, interval: r.schedule?.interval ?? 1, time: r.schedule?.time ?? blank.time, after: !!r.after, afterBot: r.after?.bot ?? "", afterOutcome: r.after?.outcome ?? blank.afterOutcome, webhook: r.webhook, chat: r.chat, precheck: r.precheck ?? "", watching: !!r.watch, connector: r.watch?.connector ?? "", tool: r.watch?.tool ?? "", args: JSON.stringify(r.watch?.arguments ?? {}), every: r.watch?.every ?? blank.every });

/** A device on the network may not set a precheck or a watch: its frames leave them out and the service keeps the ones there are. */
const frame = (r: Routine): Routine => (inShell ? r : { ...r, precheck: undefined, watch: undefined });

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
              {x.by === "after" && <small>Started after another bot</small>}
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
  const [bots, setBots] = useState<Bot[]>([]);
  const { later } = useLater(bot);
  const [connectors, setConnectors] = useState<Connector[]>([]);
  const [permissions, setPermissions] = useState<Permissions>();
  const tools = watchTools(connectors.find((c) => c.name === form.connector), form.tool, permissions);

  useEffect(() => {
    const load = () =>
      request<{ routines: Routine[] }>("list_routines", { bot }).then((d) => (setRoutines(d.routines), setError("")), (e: Error) => setError(e.message));
    load();
    return listen((frame) => {
      if (frame.type === "routines_changed" && frame.bot === bot) load();
    });
  }, [bot]);

  useEffect(() => {
    request<{ bots: Bot[] }>("list_bots").then((d) => setBots(d.bots), () => {});
  }, []);

  /** The bot's connectors, for the dialog's choice of what to watch. */
  useEffect(() => {
    if (ask && inShell) {
      void request<{ connectors: Connector[] }>("list_connectors", { bot }).then((d) => setConnectors(d.connectors), () => setConnectors([]));
      void request<Permissions>("get_permissions", { bot }).then(setPermissions, () => setPermissions(undefined));
    }
  }, [ask, bot]);

  /** Whether it worked; if not, the error shows. */
  const run = (p: Promise<unknown>, done = false) =>
    p.then(() => (setError(""), setSaved(done), true), (e: Error) => (setError(e.message), setSaved(false), false));
  const open = (edit?: Routine) => (setForm(edit ? formOf(edit) : blank), setAsk({ edit }));
  const save = async () => {
    const { edit } = ask!;
    const { name, prompt, scheduled, frequency, interval, time, after, afterBot, afterOutcome, webhook, chat, precheck, watching, connector, tool, args, every } = form;
    if (!scheduled && !after && !watching && !webhook && !chat) throw new Error("Choose what starts it: a schedule, another bot, a change in a connected app, a web address or a chat.");
    if (after && !afterBot) throw new Error("Choose the bot it follows.");
    await request("set_routine", {
      bot,
      routine: frame({ ...edit, name, prompt, enabled: edit?.enabled ?? true, webhook, chat, precheck: precheck.trim() || undefined, schedule: scheduled ? { ...edit?.schedule, frequency, interval, time } : undefined, after: after ? { bot: afterBot, outcome: afterOutcome } : undefined, watch: watching ? { connector, tool: tool.trim(), arguments: watchArguments(args), every } : undefined }),
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
                  <small>{r.watch ? watchText(r.watch) : r.after ? afterText(r.after, bots.find((b) => b.id === r.after!.bot)?.name) : when(r)}</small>
                  {r.lastCheck && <small className={r.lastCheck.error ? "error" : undefined}>{checkText(r.lastCheck)}</small>}
                  <small>
                    {r.enabled && (r.backoff?.nextRunMs ?? r.nextRunMs) !== undefined && `${r.watch ? "Next check" : "Next"}: ${dayTime((r.backoff?.nextRunMs ?? r.nextRunMs)!)}`}
                    {r.enabled && (r.backoff?.nextRunMs ?? r.nextRunMs) !== undefined && r.lastRun && " · "}
                    {r.lastRun && `Last: ${dayTime(r.lastRun.atMs)}${r.lastRun.by === "user" ? " (started by you)" : r.lastRun.by === "after" ? " (after another bot)" : r.lastRun.by === "watch" ? " (a change)" : ""}`}
                  </small>
                  {r.lastRun?.error && <small className="error">{r.lastRun.error}</small>}
                  {r.lastRun && skipWhy(r.lastRun) && <small>{skipWhy(r.lastRun)}</small>}
                  {r.lastRun?.tokens && <small>Used {tokenCount(r.lastRun.tokens)}</small>}
                  {r.lastRun?.reason === "signed_out" && r.lastRun.detail && <button className="small" onClick={() => void run(request<{ url: string }>("sign_in_connector", { bot, name: r.lastRun!.detail }).then((d) => openInBrowser(d.url)))}>Sign in to {r.lastRun.detail}</button>}
                  {r.precheck && <small>Checks first: <code>{r.precheck}</code></small>}
                  {r.chat && <small>From a paired chat: write /run {r.name}</small>}
                  {r.hookUrl && <small>Web address: <code>{r.hookUrl}</code></small>}
                </div>
                <div className="actions">
                  <button className="small" onClick={() => void run(request("run_routine", { bot, routineId: r.id }))}>Test</button>
                  <label>
                    <input type="checkbox" role="switch" aria-label={r.name} checked={r.enabled} onChange={(e) => void run(request("set_routine", { bot, routine: frame({ ...r, enabled: e.target.checked }) }), true)} />
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
      {later.length > 0 && (
        <>
          <h3>Later turns</h3>
          <p className="note">Turns the bot set for itself. Each runs once, when it is due, or when the bot is free after that.</p>
          <ul className="rows">
            {later.map((l) => (
              <li key={l.id}>
                <div className="what">
                  <strong>{dayTime(l.dueMs)}</strong>
                  <small>{l.note}</small>
                </div>
                <div className="actions">
                  <button className="small" aria-label={`Cancel the later turn: ${l.note}`} onClick={() => void run(request("cancel_later", { bot, later: l.id }))}>Cancel</button>
                </div>
              </li>
            ))}
          </ul>
        </>
      )}
      {ask && (
        <Sheet title={ask.edit ? "Edit routine" : "New routine"} action={ask.edit ? "Save" : "Add"} go={save} close={() => setAsk(undefined)}>
          <label>Name <input required maxLength={80} value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} /></label>
          <label>Prompt <textarea required rows={4} maxLength={8000} value={form.prompt} onChange={(e) => setForm({ ...form, prompt: e.target.value })} /></label>
          <>
            <h3>What starts it</h3>
            <label><input type="checkbox" checked={form.scheduled} onChange={(e) => setForm({ ...form, scheduled: e.target.checked, after: e.target.checked ? false : form.after, watching: e.target.checked ? false : form.watching })} /> A schedule</label>
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
            <label><input type="checkbox" checked={form.after} onChange={(e) => setForm({ ...form, after: e.target.checked, scheduled: e.target.checked ? false : form.scheduled, watching: e.target.checked ? false : form.watching })} /> After another bot</label>
            {form.after && (
              <>
                <div className="field">
                  <label htmlFor="routine-after">Bot</label>
                  <select id="routine-after" value={form.afterBot} onChange={(e) => setForm({ ...form, afterBot: e.target.value })}>
                    <option value="">Choose a bot</option>
                    {bots.filter((b) => b.id !== bot).map((b) => <option key={b.id} value={b.id}>{b.name}</option>)}
                  </select>
                </div>
                <div className="field">
                  <label htmlFor="routine-outcome">When its turn</label>
                  <select id="routine-outcome" value={form.afterOutcome} onChange={(e) => setForm({ ...form, afterOutcome: e.target.value as After["outcome"] })}>
                    <option value="done">finishes</option>
                    <option value="failed">fails</option>
                    <option value="any">ends either way</option>
                  </select>
                </div>
                <p className="note">The routine gets the bot's last line in front of its prompt. A chain of these starts stops after 3 steps, and the routine starts at most 20 times an hour. A bot that is paused or busy is skipped.</p>
              </>
            )}
            <label><input type="checkbox" checked={form.webhook} onChange={(e) => setForm({ ...form, webhook: e.target.checked })} /> A web address</label>
            {form.webhook && <p className="note">The address is shown under the routine. Anyone on this Mac who knows it can start the routine. For a new address, turn this off and save, then turn it on and save.</p>}
            <label><input type="checkbox" checked={form.chat} onChange={(e) => setForm({ ...form, chat: e.target.checked })} /> A paired chat (Telegram, Discord), by writing /run and its name</label>
            {(inShell || form.watching) && <label><input type="checkbox" checked={form.watching} disabled={!inShell} onChange={(e) => setForm({ ...form, watching: e.target.checked, scheduled: e.target.checked ? false : form.scheduled, after: e.target.checked ? false : form.after })} /> A change in a connected app</label>}
            {form.watching && inShell && (
              <>
                <label>
                  Connector
                  <select required value={form.connector} onChange={(e) => setForm({ ...form, connector: e.target.value, tool: "" })}>
                    <option value="" disabled>Choose…</option>
                    {[...new Set([...connectors.filter((c) => c.enabled).map((c) => c.name), ...(form.connector ? [form.connector] : [])])].map((n) => <option key={n} value={n}>{n}</option>)}
                  </select>
                </label>
                {tools ? (
                  <label>
                    Tool
                    <select required value={form.tool} onChange={(e) => setForm({ ...form, tool: e.target.value })}>
                      <option value="" disabled>Choose…</option>
                      {tools.map((t) => <option key={t.own} value={t.own}>{t.label}</option>)}
                    </select>
                  </label>
                ) : (
                  <>
                    <label>Tool <input required maxLength={64} value={form.tool} onChange={(e) => setForm({ ...form, tool: e.target.value })} /></label>
                    {form.connector && <p className="note">{form.connector} has not been loaded yet, so its tools are not known. Type the tool's name; its tools are listed here once the bot has used the connector.</p>}
                  </>
                )}
                <label>Arguments, as JSON <textarea rows={2} value={form.args} onChange={(e) => setForm({ ...form, args: e.target.value })} /></label>
                <label>Look every, in minutes <input type="number" min={5} max={1440} required value={form.every} onChange={(e) => setForm({ ...form, every: Number(e.target.value) })} /></label>
                <p className="note">Bate calls the tool itself, without a model, and starts the routine when the answer is not the one before, with the answer in front of the prompt. The first answer is only kept. The bot must be able to use this tool without asking (its level for connectors is Always, or the tool is allowed) and it must not be blocked; a tool that would ask is refused. A call that fails never starts the routine.</p>
              </>
            )}
            {form.watching && !inShell && <p className="note">Watches {form.connector}: {form.tool}. Only Bate's own window can change it.</p>}
            {inShell ? (
              <>
                <label>Check first, with a command <input maxLength={500} value={form.precheck} onChange={(e) => setForm({ ...form, precheck: e.target.value })} /></label>
                {form.precheck.trim() && <p className="note">Runs before each run, without a model, in the bot's folder and under its limits. If it ends with 0, the bot runs and gets what it printed; any other end skips the run. Saving allows exactly this command for this routine, without asking again.</p>}
              </>
            ) : (
              form.precheck && <p className="note">Checks first: <code>{form.precheck}</code>. Only Bate's own window can change it.</p>
            )}
          </>
        </Sheet>
      )}
    </section>
  );
}
