import { cloneElement, type ReactElement, type ReactNode, useRef, useState } from "react";

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

/** A switch row: a change that takes effect at once, its words in the label, a hint or the reason it is off under it. */
export function Toggle({ label, hint, error, saved, checked, disabled, onChange }: { label: string; hint?: ReactNode; error?: string; saved?: boolean; checked: boolean; disabled?: boolean; onChange: (on: boolean) => void }) {
  return (
    <div className="field">
      <label>
        <input type="checkbox" role="switch" checked={checked} disabled={disabled} onChange={(e) => onChange(e.target.checked)} /> {label}
      </label>{" "}
      {saved !== undefined && <Saved on={saved} />}
      {hint && <small>{hint}</small>}
      <Problem text={error} />
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
