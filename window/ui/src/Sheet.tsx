import { type ReactNode, useState } from "react";
import { Avatar } from "./Avatar.tsx";
import { byLine } from "./addons.ts";
import { Dialog } from "./Dialog.tsx";
import type { Bot } from "./types.ts";

/** Who or what an add-on goes to, or is taken from: a bot, or a project. */
export type Who = {
  legend: string;
  items: { id: string; bot?: Bot; label: string; note?: string; off?: boolean }[];
  /** Ticked when the sheet opens. */
  ticked?: string[];
  /** Nothing ticked is allowed. */
  optional?: boolean;
};

/**
 * The one sheet for installing, adding to more bots and removing: what it is, who made it, what it may
 * do, and who gets it. `go` runs with the ticked ids and says which of them failed; the sheet closes
 * when none did, and else stays with those ticked, so a second try repeats only them.
 */
export function Sheet(p: {
  title: string;
  about?: string;
  maker?: string;
  /** What the add-on's own file says about who made it: shown as a claim. */
  author?: string;
  facts?: string[];
  may?: string[];
  /** What the person agrees to before anything is fetched. */
  agree?: string;
  /** Fields the sheet has of its own (a new connector's). */
  children?: ReactNode;
  who?: Who;
  action: string;
  danger?: boolean;
  /** What is happening while it runs (a download's progress). */
  status?: string;
  go: (ids: string[]) => Promise<{ id: string; error: string }[] | void>;
  close: () => void;
}) {
  const [ticked, setTicked] = useState(p.who?.ticked ?? []);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const ids = p.who?.items.filter((i) => !i.off).map((i) => i.id) ?? [];

  const submit = async () => {
    setBusy(true);
    const failed = (await p.go(ticked).catch((e: Error) => [{ id: "", error: e.message }])) ?? [];
    setBusy(false);
    if (!failed.length) return p.close();
    const label = (id: string) => p.who?.items.find((i) => i.id === id)?.label;
    setError(failed.map((f) => (label(f.id) ? `${label(f.id)}: ${f.error}` : f.error)).join("\n"));
    if (p.who) setTicked(failed.map((f) => f.id).filter((id) => id));
  };
  const list = (title: string, lines?: string[]) =>
    lines?.length ? (
      <>
        <h3>{title}</h3>
        <ul>{lines.map((l) => <li key={l}>{l}</li>)}</ul>
      </>
    ) : null;

  return (
    <Dialog labelledby="sheet-title" close={p.close}>
      <form className="fields stack" onSubmit={(e) => { e.preventDefault(); void submit(); }}>
        <h2 id="sheet-title">{p.title}</h2>
        {p.about && <p>{p.about}</p>}
        {p.children}
        {(p.maker || p.author) && (
          <>
            <h3>Made by</h3>
            {p.maker && <p>{p.maker}</p>}
            {p.author && <p>{byLine(p.author)}</p>}
          </>
        )}
        {list("What it is", p.facts)}
        {list("What it may do", p.may)}
        {p.agree && <p>{p.agree} <strong>Nothing is downloaded until you agree.</strong></p>}
        {p.who && (
          <fieldset>
            <legend>{p.who.legend}</legend>
            {p.who.items.length === 0 && <p className="note">None yet.</p>}
            <ul className="rows">
              {p.who.items.map((i) => (
                <li key={i.id}>
                  <label className="row">
                    <input
                      type="checkbox"
                      disabled={i.off}
                      checked={ticked.includes(i.id)}
                      onChange={(e) => setTicked(e.target.checked ? [...ticked, i.id] : ticked.filter((t) => t !== i.id))}
                    />
                    {i.bot && <Avatar bot={i.bot} />}
                    <span className="label">
                      <span>{i.label}</span>
                      {i.note && <small>{i.note}</small>}
                    </span>
                  </label>
                </li>
              ))}
            </ul>
            {ids.length > 1 && (
              <button type="button" className="small" onClick={() => setTicked(ticked.length === ids.length ? [] : ids)}>
                {ticked.length === ids.length ? "Select none" : "Select all"}
              </button>
            )}
          </fieldset>
        )}
        {p.status && <p className="note" role="status">{p.status}</p>}
        {error && <p className="error" role="alert">{error}</p>}
        <div className="actions">
          <button className={p.danger ? "danger" : undefined} disabled={busy || (!!p.who && !p.who.optional && ticked.length === 0)}>
            {p.action}
          </button>
          <button type="button" onClick={p.close}>Cancel</button>
        </div>
      </form>
    </Dialog>
  );
}
