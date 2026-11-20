import { type ReactNode, useEffect, useRef, useState } from "react";

/**
 * A question before something that cannot be undone, in a dialog. Cancel holds the focus, so that a stray
 * Enter does not do it. `go` runs on the confirm button; a failure stays in the dialog, a success closes it.
 */
export function Confirm({ title, children, action, go, close }: { title: string; children: ReactNode; action: string; go: () => Promise<unknown>; close: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const cancel = useRef<HTMLButtonElement>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    // The focus goes in and comes back to the button that opened the dialog.
    const before = document.activeElement as HTMLElement | null;
    const d = dialog.current!;
    if (!d.open) d.showModal();
    cancel.current?.focus();
    return () => before?.focus();
  }, []);
  return (
    <dialog ref={dialog} className="sheet" aria-labelledby="confirm-title" onClose={close} onClick={(e) => e.target === e.currentTarget && close()}>
      <form
        className="fields stack"
        onSubmit={(e) => {
          e.preventDefault();
          setBusy(true);
          go().then(close, (err: Error) => (setError(err.message), setBusy(false)));
        }}
      >
        <h2 id="confirm-title">{title}</h2>
        {children}
        {error && <p className="error" role="alert">{error}</p>}
        <div className="actions">
          <button className="danger" disabled={busy}>{action}</button>
          <button type="button" ref={cancel} onClick={close}>Cancel</button>
        </div>
      </form>
    </dialog>
  );
}
