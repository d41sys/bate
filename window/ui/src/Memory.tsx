import { useEffect, useId, useRef, useState } from "react";
import { listen, request } from "./service.ts";
import { type Editor, firstLine, keep, kept, size, unsaved } from "./memory.ts";

type Note = { name: string; bytes: number; modifiedMs: number; first: string };
const NONE: Editor = { open: null, name: "", text: "", saved: "" };

/** Whose notes: a bot's own, or those of a project, which all its bots share. */
export function Memory({ target, title }: { target: { bot: string } | { projectId: string }; title: string }) {
  const id = "bot" in target ? `bot:${target.bot}` : `project:${target.projectId}`;
  const [notes, setNotes] = useState<Note[]>();
  // What was typed and not saved when the screen was left: it is back, and says so.
  const [draft] = useState(() => kept(id));
  const [editor, setEditor] = useState<Editor>(draft ?? NONE);
  const [restored, setRestored] = useState(!!draft);
  // The note just saved.
  const [saved, setSaved] = useState("");
  const [deleting, setDeleting] = useState("");
  // `at`: the note it came from, "" for the new one, "*" for the list.
  const [error, setError] = useState<{ at: string; text: string }>();
  const dirty = unsaved(editor);
  const heading = useId();
  const area = useRef<HTMLTextAreaElement>(null);
  const add = useRef<HTMLButtonElement>(null);
  useEffect(() => keep(id, editor), [id, editor]);

  // shortcut: reads every note for its first line; fine for tens of notes, give `list_memory` the line if bots write hundreds.
  const load = () =>
    request<{ notes: Omit<Note, "first">[] }>("list_memory", target)
      .then((d) =>
        Promise.all(d.notes.map((n) => request<{ text: string }>("read_memory", { ...target, name: n.name }).then((r) => firstLine(r.text), () => ""))).then((firsts) =>
          setNotes(d.notes.map((n, i) => ({ ...n, first: firsts[i] }))),
        ),
      )
      .then(
        () => setError(undefined),
        (e: Error) => setError({ at: "*", text: e.message }),
      );
  useEffect(() => {
    void load();
    return listen((frame) => {
      if (frame.type !== "memory_changed") return;
      if ("bot" in target ? frame.bot === target.bot : frame.projectId === target.projectId) void load();
    });
  }, [id]);

  const put = (e: Editor) => (setEditor(e), setRestored(false), setSaved(""), setError(undefined));
  const read = async (n: string) => {
    try {
      const d = await request<{ text: string }>("read_memory", { ...target, name: n });
      put({ open: n, name: n, text: d.text, saved: d.text });
    } catch (e) {
      setError({ at: n, text: (e as Error).message });
    }
  };
  // The bot's own writes are not announced: this asks again.
  const reload = () => {
    void load();
    if (editor.open && !dirty) void read(editor.open);
  };
  const save = async () => {
    const { name, text } = editor;
    try {
      await request("write_memory", { ...target, name, text });
    } catch (e) {
      return setError({ at: editor.open!, text: (e as Error).message });
    }
    // The list has the note before the editor moves to its row.
    await load();
    put({ open: name, name, text, saved: text });
    setSaved(name);
    area.current?.focus();
  };
  const discard = () => {
    if (editor.open === "") {
      put(NONE);
      add.current?.focus();
    } else {
      put({ ...editor, text: editor.saved });
      area.current?.focus();
    }
  };
  const remove = async (n: string) => {
    setDeleting("");
    try {
      await request("delete_memory", { ...target, name: n });
    } catch (e) {
      return setError({ at: n, text: (e as Error).message });
    }
    if (n === editor.open) put(NONE);
    void load();
  };

  // A note opened and then gone from the folder still shows its row, so its text is not out of sight.
  const rows = notes && editor.open && !notes.some((n) => n.name === editor.open) ? [...notes, { name: editor.open, bytes: 0, modifiedMs: 0, first: "" }] : notes;
  const empty = rows?.length === 0 && editor.open !== "";
  const hint = `${heading}-hint`;
  const errorAt = (at: string) => error?.at === at && <p className="error" role="alert">{error.text}</p>;
  const form = (
    <form className="fields stack" aria-label={editor.open || "New note"} onSubmit={(e) => { e.preventDefault(); void save(); }}>
      {editor.open === "" && (
        <label>
          Name
          <input required autoFocus aria-describedby={hint} value={editor.name} onChange={(e) => setEditor({ ...editor, name: e.target.value })} />
          <small id={hint}>A file name ending in .md, at most 100 bytes, not starting with a dot, without /.</small>
        </label>
      )}
      <label>
        Text
        <textarea ref={area} autoFocus={editor.open !== ""} rows={12} value={editor.text} onChange={(e) => setEditor({ ...editor, text: e.target.value })} />
        <small>A note is at most 64 KiB.</small>
      </label>
      <div className="actions">
        <button disabled={!dirty && editor.open !== ""}>Save</button>
        {dirty && <button type="button" onClick={discard}>Discard changes</button>}
        <span className="note" role="status">{saved === editor.open && !dirty ? "Saved" : ""}</span>
        {dirty && <span className="note">Unsaved{restored ? ", kept from before you left this tab" : ""}. Save or discard before opening another note.</span>}
      </div>
      {errorAt(editor.open!)}
    </form>
  );

  return (
    <section aria-labelledby={heading}>
      <h2 id={heading}>{title}</h2>
      <p className="note">
        {"bot" in target
          ? "Only this bot reads them: at the start of every turn, in every conversation. It adds its own."
          : "Every bot in this project reads them, after its own notes, and can add to them. They are files in the project's folder."}
      </p>
      {errorAt("*")}
      {!rows ? (
        !error && <p className="note">Loading notes…</p>
      ) : empty ? (
        <div className="empty">
          <p>No notes yet. {"bot" in target ? "The bot writes its own" : "The project's bots write them"}, or add one.</p>
          <button ref={add} onClick={() => put({ ...NONE, open: "" })}>New note</button>
        </div>
      ) : (
        <ul className="rows">
          {editor.open === "" && <li>{form}</li>}
          {rows.map((n) => {
            const isOpen = editor.open === n.name;
            return (
              <li key={n.name}>
                <div className="what">
                  <strong>{n.first || n.name}</strong>
                  <small>
                    {n.first ? `${n.name} · ` : ""}
                    {n.modifiedMs ? `${size(n.bytes)} · changed ${new Date(n.modifiedMs).toLocaleString()}` : "not in the list"}
                  </small>
                </div>
                <div className="actions">
                  {deleting === n.name ? (
                    <>
                      <span>Delete {n.name}?</span>
                      <button className="small danger" onClick={() => void remove(n.name)}>Delete</button>
                      <button className="small" autoFocus onClick={() => setDeleting("")}>Cancel</button>
                    </>
                  ) : (
                    <>
                      <button className="small" disabled={dirty} aria-expanded={isOpen} aria-label={`${isOpen ? "Close" : "Open"} ${n.name}`} onClick={() => (isOpen ? put(NONE) : void read(n.name))}>
                        {isOpen ? "Close" : "Open"}
                      </button>
                      <button className="small danger" aria-label={`Delete ${n.name}`} onClick={() => setDeleting(n.name)}>Delete</button>
                    </>
                  )}
                </div>
                {isOpen ? form : errorAt(n.name)}
              </li>
            );
          })}
        </ul>
      )}
      <div className="actions">
        {rows && !empty && <button className="small" ref={add} disabled={dirty} onClick={() => put({ ...NONE, open: "" })}>New note</button>}
        <button className="small ghost" onClick={reload}>Reload</button>
      </div>
    </section>
  );
}
