import { useEffect, useState } from "react";
import { listen, request } from "./service.ts";
import { size } from "./memory.ts";

type Note = { name: string; bytes: number; modifiedMs: number };

/** Whose notes: a bot's own, or those of a project, which all its bots share. */
export function Memory({ target }: { target: { bot: string } | { projectId: string } }) {
  const [notes, setNotes] = useState<Note[]>([]);
  // The note in the editor; empty: a new one, named in the field.
  const [open, setOpen] = useState("");
  const [name, setName] = useState("");
  const [text, setText] = useState("");
  const [saved, setSaved] = useState("");
  const [deleting, setDeleting] = useState("");
  const [error, setError] = useState("");
  // Typed text is never replaced: while it is unsaved, nothing else can be opened.
  const dirty = text !== saved;

  const load = () =>
    request<{ notes: Note[] }>("list_memory", target).then((d) => setNotes(d.notes), (e) => setError(e.message));
  useEffect(() => {
    load();
    return listen((frame) => {
      if (frame.type !== "memory_changed") return;
      if ("bot" in target ? frame.bot === target.bot : frame.projectId === target.projectId) load();
    });
  }, ["bot" in target ? target.bot : target.projectId]);

  /** Whether it worked; if not, the error shows. */
  const run = (p: Promise<unknown>) =>
    p.then(() => (setError(""), true), (e: Error) => (setError(e.message), false));
  const show = (n: string, t: string) => (setOpen(n), setName(n), setText(t), setSaved(t));
  const read = async (n: string) => {
    try {
      const d = await request<{ text: string }>("read_memory", { ...target, name: n });
      show(n, d.text);
      setError("");
    } catch (e) {
      setError((e as Error).message);
    }
  };
  // The bot's own writes are not announced: this asks again.
  const reload = () => {
    load();
    if (open && !dirty) read(open);
  };
  const save = async () => {
    if (await run(request("write_memory", { ...target, name, text }))) {
      show(name, text);
      load();
    }
  };
  const remove = async (n: string) => {
    setDeleting("");
    if (await run(request("delete_memory", { ...target, name: n }))) {
      if (n === open) show("", "");
      load();
    }
  };

  return (
    <section>
      <button onClick={reload}>Reload</button>
      {notes.length === 0 ? (
        <p>No notes yet. {"bot" in target ? "The bot writes its own" : "The project's bots write them"}, or add one below.</p>
      ) : (
        <ul className="rows">
          {notes.map((n) => (
            <li key={n.name}>
              <button disabled={dirty} onClick={() => read(n.name)} aria-label={`Open ${n.name}`}>{n.name}</button>
              <span className="note">{size(n.bytes)} · changed {new Date(n.modifiedMs).toLocaleString()}</span>
              {deleting === n.name ? (
                <span>
                  Delete {n.name}?{" "}
                  <button onClick={() => remove(n.name)}>Delete</button>{" "}
                  <button onClick={() => setDeleting("")}>Cancel</button>
                </span>
              ) : (
                <button onClick={() => setDeleting(n.name)} aria-label={`Delete ${n.name}`}>Delete</button>
              )}
            </li>
          ))}
        </ul>
      )}
      <form className="fields" onSubmit={(e) => { e.preventDefault(); save(); }}>
        <h2>{open ? open : "New note"}</h2>
        {!open && (
          <label>
            Name <input required value={name} onChange={(e) => setName(e.target.value)} />
            <span className="note"> A file name ending in .md, at most 100 bytes, not starting with a dot, without /. A note is at most 64 KiB.</span>
          </label>
        )}
        <label>Text <textarea rows={12} value={text} onChange={(e) => setText(e.target.value)} /></label>
        <button disabled={!dirty && !!open}>Save</button>{" "}
        {dirty && <button type="button" onClick={() => setText(saved)}>Discard changes</button>}
        {open && !dirty && <button type="button" onClick={() => show("", "")}>New note</button>}
        {dirty && <span className="note"> Unsaved: save or discard before opening another note.</span>}
      </form>
      {error && <p className="error" role="alert">{error}</p>}
    </section>
  );
}
