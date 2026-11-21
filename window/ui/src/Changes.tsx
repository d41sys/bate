import { Fragment, useEffect, useMemo, useRef, useState } from "react";
import { request } from "./service.ts";
import { type ChangedFile, type Comment, type Listed, type Row, type Shown, commentOn, countLines, covers, loadComments, message, parseDiff, saveComments, where } from "./changes.ts";
import type { Bot } from "./types.ts";

/** A list this short has its files open at first. */
const OPEN_AT_FIRST = 5;

const STATUS = { added: "New", modified: "Changed", deleted: "Removed" } as const;

/**
 * What a bot changed in its folder, as a diff for each file; read only. A line is clicked (or a few are dragged over,
 * or shift-clicked) to write a comment on it. The comments are kept until they are sent, as one message to the bot, or discarded.
 */
export function Changes({ bot, conversation }: { bot: Bot; conversation: string }) {
  const [listed, setListed] = useState<Listed>();
  const [error, setError] = useState("");
  // Reload takes it up: the files are listed and read again.
  const [round, setRound] = useState(0);
  const [comments, setComments] = useState(() => loadComments(bot.id, conversation));
  const [sending, setSending] = useState(false);
  const [sent, setSent] = useState(false);
  const [failed, setFailed] = useState("");

  useEffect(() => {
    let gone = false;
    request<Listed>("list_changes", { bot: bot.id, conversation }).then(
      (d) => gone || (setListed(d), setError("")),
      (e: Error) => gone || setError(e.message),
    );
    return () => {
      gone = true;
    };
  }, [bot.id, conversation, round]);

  const keep = (next: Comment[]) => {
    setComments(next);
    saveComments(bot.id, conversation, next);
    setSent(false);
  };
  const send = () => {
    setSending(true);
    request("prompt", { bot: bot.id, conversation, nonce: crypto.randomUUID(), text: message(comments) }).then(
      () => {
        keep([]);
        setSent(true);
        setFailed("");
      },
      (e: Error) => setFailed(e.message),
    ).finally(() => setSending(false));
  };

  return (
    <div className="changes">
      <div className="tools">
        <p className="note" role="status">
          {error || !listed
            ? error || "Loading the changes…"
            : listed.source === "git"
              ? "From Git: what is not committed yet in the bot's folder. Files that Git ignores are not shown."
              : "From the copies Bate keeps of what the bot's own write and edit replaced."}
        </p>
        <button type="button" className="small" onClick={() => setRound(round + 1)}>Reload</button>
      </div>
      {error && <p className="error" role="alert">{error}</p>}
      {listed?.source === "copies" && <p className="banner">This list may be incomplete: files that the bot changed with a command, or through an installed agent, are not in it.</p>}
      {listed && !listed.git && <p className="banner">Git is not installed on this Mac. A folder under Git is not listed from Git, and a changed file cannot be compared with its earlier version.</p>}
      {listed?.files.length === 0 && <p className="empty">No changes to show.</p>}
      <ul className="files">
        {listed?.files.map((file) => (
          <File
            key={`${round}:${file.path}`}
            bot={bot.id}
            conversation={conversation}
            file={file}
            open={listed.files.length <= OPEN_AT_FIRST || comments.some((c) => c.path === file.path)}
            comments={comments.filter((c) => c.path === file.path)}
            add={(c) => keep([...comments, c])}
          />
        ))}
      </ul>
      {listed?.more && <p className="note">Only the first files are listed.</p>}
      {sent && <p className="note" role="status">Sent to the bot.</p>}
      {comments.length > 0 && (
        <section className="comments" aria-label="Comments">
          <details>
            <summary>{comments.length === 1 ? "1 comment" : `${comments.length} comments`}</summary>
            <ul className="rows">
              {comments.map((c) => (
                <li key={c.id}>
                  <div className="what">
                    <strong>{c.path}</strong>
                    <small>{where(c)}</small>
                    <small>{c.text}</small>
                  </div>
                  <div className="actions">
                    <button type="button" className="small" aria-label={`Remove the comment on ${c.path}, ${where(c)}`} onClick={() => keep(comments.filter((x) => x !== c))}>Remove</button>
                  </div>
                </li>
              ))}
            </ul>
          </details>
          {failed && <p className="error" role="alert">{failed}</p>}
          <div className="actions">
            <button type="button" className="primary" disabled={sending} onClick={send}>Send comments to the bot</button>
            <button type="button" disabled={sending} onClick={() => keep([])}>Discard</button>
          </div>
        </section>
      )}
    </div>
  );
}

/** One file of the list: a summary that opens to its diff, which is read when it first opens. */
function File({ bot, conversation, file, open: first, comments, add }: {
  bot: string;
  conversation: string;
  file: ChangedFile;
  open: boolean;
  comments: Comment[];
  add: (c: Comment) => void;
}) {
  const [open, setOpen] = useState(first);
  const [shown, setShown] = useState<Shown>();
  const [error, setError] = useState("");
  useEffect(() => {
    if (!open || shown || error) return;
    request<Shown>("read_change", { bot, conversation, path: file.path }).then(setShown, (e: Error) => setError(e.message));
  }, [open]);
  const rows = useMemo(() => (shown?.kind === "text" ? parseDiff(shown.diff) : []), [shown]);
  const { added, removed } = countLines(rows);
  return (
    <li>
      <details open={open} onToggle={(e) => setOpen(e.currentTarget.open)}>
        <summary>
          <code>{file.path}</code>
          <span className="badge quiet">{STATUS[file.status]}</span>
          {rows.length > 0 && <small>+{added} −{removed}</small>}
          {comments.length > 0 && <small>{comments.length === 1 ? "1 comment" : `${comments.length} comments`}</small>}
        </summary>
        {error ? (
          <p className="error" role="alert">{error}</p>
        ) : !shown ? (
          open && <p className="note">Loading…</p>
        ) : shown.kind === "binary" ? (
          <p className="note">A binary file: not shown.</p>
        ) : shown.kind === "large" ? (
          <p className="note">A large file: not shown.</p>
        ) : rows.length === 0 ? (
          <p className="note">{shown.kind === "none" ? "Nothing to show: the file is as it was, or is not a plain file." : "No lines to show."}</p>
        ) : (
          <Diff path={file.path} rows={rows} comments={comments} add={add} />
        )}
      </details>
    </li>
  );
}

type Range = { from: number; to: number; anchor: number };

/** The lines of a diff, to comment on: click one, drag over some, or shift-click the end of a range. */
function Diff({ path, rows, comments, add }: { path: string; rows: Row[]; comments: Comment[]; add: (c: Comment) => void }) {
  // The lines being written about, and what is written so far.
  const [range, setRange] = useState<Range>();
  const [text, setText] = useState("");
  // A drag with the mouse in progress, from the number of the row it began at.
  const [drag, setDrag] = useState<{ from: number; to: number }>();
  const pick = (from: number, to: number, anchor = from) => setRange({ from: Math.min(from, to), to: Math.max(from, to), anchor });
  // The box for the comment comes into view when it opens or moves, as far as the list can scroll.
  const table = useRef<HTMLTableElement>(null);
  useEffect(() => {
    table.current?.querySelector(".compose")?.scrollIntoView({ block: "nearest" });
  }, [range?.from, range?.to]);
  useEffect(() => {
    if (!drag) return;
    const end = () => {
      if (drag.from !== drag.to) pick(drag.from, drag.to);
      setDrag(undefined);
    };
    addEventListener("pointerup", end, { once: true });
    return () => removeEventListener("pointerup", end);
  }, [drag]);

  const chosen = drag ? { from: Math.min(drag.from, drag.to), to: Math.max(drag.from, drag.to) } : range;
  const about = range && commentOn(path, rows, range.from, range.to, "");
  const close = () => {
    setRange(undefined);
    setText("");
  };

  return (
    <table ref={table} className="diff" aria-label={`Changes in ${path}`}>
      <tbody>
        {rows.map((row, i) =>
          row.kind === "hunk" ? (
            <tr key={i} className="hunk"><td colSpan={2}>{row.text}</td></tr>
          ) : (
            <Fragment key={i}>
              <tr
                className={`${row.kind}${chosen && i >= chosen.from && i <= chosen.to ? " chosen" : ""}${comments.some((c) => covers(c, row)) ? " marked" : ""}`}
                onPointerEnter={() => drag && setDrag({ ...drag, to: i })}
                onClick={(e) => {
                  // Words being selected in the code are not a click on the line.
                  if (!getSelection()?.isCollapsed) return;
                  if (e.shiftKey && range) pick(range.anchor, i);
                  else pick(i, i);
                }}
              >
                <td className="no" onPointerDown={(e) => e.button === 0 && e.pointerType === "mouse" && setDrag({ from: i, to: i })}>
                  <button type="button" aria-label={`Comment on ${row.kind === "del" ? "removed " : ""}line ${row.now ?? row.old}`}>{row.now ?? row.old}</button>
                </td>
                <td className="text">{row.kind === "add" ? "+" : row.kind === "del" ? "-" : " "}{row.text}</td>
              </tr>
              {range?.to === i && about && (
                <tr className="compose">
                  <td colSpan={2}>
                    <form
                      onSubmit={(e) => {
                        e.preventDefault();
                        const made = text.trim() && commentOn(path, rows, range.from, range.to, text.trim());
                        if (made) add(made);
                        close();
                      }}
                    >
                      <label>
                        Comment on {where(about)}
                        <textarea
                          autoFocus
                          rows={2}
                          value={text}
                          onChange={(e) => setText(e.target.value)}
                          onKeyDown={(e) => {
                            // Esc ends the comment, not the pane.
                            if (e.key === "Escape") {
                              e.stopPropagation();
                              close();
                            }
                          }}
                        />
                      </label>
                      <div className="actions">
                        <button disabled={!text.trim()}>Add comment</button>
                        <button type="button" onClick={close}>Cancel</button>
                        <button type="button" className="small" disabled={range.from === 0 || rows[range.from - 1].kind === "hunk"} onClick={() => pick(range.from - 1, range.to, range.anchor)}>Line above</button>
                        <button type="button" className="small" disabled={range.to === rows.length - 1 || rows[range.to + 1].kind === "hunk"} onClick={() => pick(range.from, range.to + 1, range.anchor)}>Line below</button>
                      </div>
                    </form>
                  </td>
                </tr>
              )}
            </Fragment>
          ),
        )}
      </tbody>
    </table>
  );
}
