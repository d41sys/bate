import { cloneElement, isValidElement, type ReactElement, type ReactNode, useId, useRef, useState } from "react";

/** What a page of settings knows about its saves: the control that saved last, and each control's error, by key. */
export function useSaves() {
  const [saved, setSaved] = useState("");
  const [errors, setErrors] = useState<Record<string, string>>({});
  const fail = (key: string, message: string) => (setSaved((s) => (s === key ? "" : s)), setErrors((all) => ({ ...all, [key]: message })));
  return {
    /** Runs a change: "Saved" shows by its control until the next edit or save, an error shows by it too. */
    run: (key: string, p: Promise<unknown>) =>
      p.then(() => (setSaved(key), setErrors(({ [key]: _gone, ...rest }) => rest)), (e: Error) => fail(key, e.message)),
    /** A failure that no save caused (a list that would not load). */
    fail,
    /** The control is being edited again: its "Saved" goes. */
    edit: (key: string) => setSaved((s) => (s === key ? "" : s)),
    saved: (key: string) => saved === key,
    error: (key: string) => errors[key],
  };
}

/** "Saved", for a screen reader too: the region is always there, only its words come and go. */
export const Saved = ({ on }: { on: boolean }) => <span className="saved" role="status">{on ? "Saved" : ""}</span>;

/** A line under a control saying why it failed. */
export const Problem = ({ text }: { text?: string }) => (text ? <p className="error" role="alert">{text}</p> : null);

/** A form row: a label over its one control, a hint and an error under it. The control takes the label's id and describes itself by both. */
export function Field({ id, label, hint, error, children }: { id: string; label: string; hint?: ReactNode; error?: string; children: ReactElement<Record<string, unknown>> }) {
  const by = [hint && `${id}-hint`, error && `${id}-error`].filter(Boolean).join(" ") || undefined;
  return (
    <div className="field">
      <label htmlFor={id}>{label}</label>
      {cloneElement(children, { id, "aria-describedby": by, "aria-invalid": error ? true : undefined })}
      {hint && <small id={`${id}-hint`}>{hint}</small>}
      {error && <small id={`${id}-error`} className="error" role="alert">{error}</small>}
    </div>
  );
}

/** A group of settings: a heading over one card of rows. `id` makes the heading a place to jump to (a link, ⌘K). */
export function Group({ id, title, children }: { id?: string; title: string; children: ReactNode }) {
  const own = useId();
  const heading = id ?? own;
  return (
    <section className="group" aria-labelledby={heading}>
      <h2 id={heading} tabIndex={id ? -1 : undefined}>{title}</h2>
      <div className="group-rows">{children}</div>
    </section>
  );
}

const FIELDS = ["input", "select", "textarea"];

/**
 * One row of a group: the title and one small line on the left, the control on the right, at least 44 px high.
 * A lone input, select or textarea takes the label's id and describes itself by the hint and the error; a control
 * made of several (a box and its button) is given the id (and `aria-describedby="<id>-hint"`) by its caller.
 * With no `id` the title is plain text: the row's control is a button that names itself.
 */
export function Setting({ id, title, hint, error, saved, wide, inline, children }: {
  id?: string;
  title: string;
  hint?: ReactNode;
  error?: string;
  /** "Saved" beside the control: a change that takes effect at once says so. */
  saved?: boolean;
  /** A text of several lines: the control has the row's whole width, under the title. */
  wide?: boolean;
  /** A switch: it stays at the end of the row on a phone too. */
  inline?: boolean;
  children?: ReactNode;
}) {
  const by = id && [hint && `${id}-hint`, error && `${id}-error`].filter(Boolean).join(" ") || undefined;
  const element = isValidElement<Record<string, unknown>>(children) ? children : undefined;
  const lone = !!id && typeof element?.type === "string" && FIELDS.includes(element.type);
  // A switch or one button stays at the end of the row on a phone; a box goes under the title.
  const atEnd = inline ?? element?.type === "button";
  return (
    <div className={`setting${wide ? " wide" : ""}${atEnd ? " inline" : ""}`}>
      <div className="what">
        {id ? <label htmlFor={id}>{title}</label> : <span>{title}</span>}
        {hint && <small id={id && `${id}-hint`}>{hint}</small>}
      </div>
      <div className="control">
        {saved !== undefined && <Saved on={saved} />}
        {lone ? cloneElement(element!, { id, "aria-describedby": by, "aria-invalid": error ? true : undefined }) : children}
      </div>
      {error && <small id={id && `${id}-error`} className="error" role="alert">{error}</small>}
    </div>
  );
}

/** A code, an address or a fingerprint in a block, with a Copy button. `key` it by the text, so that "Copied" never stays on a new one. */
export function CodeBlock({ text, label }: { text: string; label: string }) {
  const pre = useRef<HTMLPreElement>(null);
  const [said, setSaid] = useState("");
  // Where the clipboard is not offered (a page served over plain http), the text is selected for the person to copy.
  const copy = () =>
    Promise.resolve()
      .then(() => navigator.clipboard.writeText(text))
      .then(
        () => setSaid("Copied"),
        () => (getSelection()?.selectAllChildren(pre.current!), setSaid("Selected: press ⌘C to copy")),
      );
  return (
    <div className="codeblock">
      <pre ref={pre}><code>{text}</code></pre>
      <button type="button" className="small" aria-label={`Copy ${label}`} onClick={copy}>Copy</button>
      <span className="saved" role="status">{said}</span>
    </div>
  );
}
