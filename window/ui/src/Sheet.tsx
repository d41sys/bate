import { type ReactNode, useEffect, useRef, useState } from "react";
import { Avatar } from "./Avatar.tsx";
import { Choose } from "./ProjectDialog.tsx";
import { inShell } from "./service.ts";
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
  const dialog = useRef<HTMLDialogElement>(null);
  const [ticked, setTicked] = useState(p.who?.ticked ?? []);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const ids = p.who?.items.filter((i) => !i.off).map((i) => i.id) ?? [];

  useEffect(() => {
    // The focus goes in and comes back to the button that opened the sheet.
    const before = document.activeElement as HTMLElement | null;
    const d = dialog.current!;
    if (!d.open) d.showModal();
    d.querySelector<HTMLElement>("input, textarea, select")?.focus();
    return () => before?.focus();
  }, []);

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
    <dialog ref={dialog} className="sheet" aria-labelledby="sheet-title" onClose={p.close} onClick={(e) => e.target === e.currentTarget && p.close()}>
      <form className="fields stack" onSubmit={(e) => { e.preventDefault(); void submit(); }}>
        <h2 id="sheet-title">{p.title}</h2>
        {p.about && <p>{p.about}</p>}
        {p.children}
        {p.maker && (
          <>
            <h3>Made by</h3>
            <p>{p.maker}</p>
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
    </dialog>
  );
}

/** A folder or a file, from the system's panel in Bate's own window; another device has no panel, so it types the whole path. */
export function Pick({ folder, pick, fail }: { folder: boolean; pick: (path: string) => void; fail: (message: string) => void }) {
  const [typed, setTyped] = useState("");
  if (inShell) return <Choose folder={folder} set={pick} fail={fail} />;
  return (
    <form
      className="find"
      onSubmit={(e) => {
        e.preventDefault();
        // The service does not expand `~`.
        if (typed.startsWith("/")) pick(typed);
        else fail("Give the whole path on the Mac, starting with /.");
      }}
    >
      <input required aria-label={`Whole path of the ${folder ? "folder" : "file"} on the Mac`} placeholder="/Users/you/Downloads/my-skill" value={typed} onChange={(e) => setTyped(e.target.value)} />
      <button>Look</button>
    </form>
  );
}
