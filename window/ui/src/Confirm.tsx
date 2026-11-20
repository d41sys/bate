import { type ReactNode, useEffect, useRef, useState } from "react";
import { Dialog } from "./Dialog.tsx";

/**
 * A question before something that cannot be undone, in a dialog. Cancel holds the focus, so that a stray
 * Enter does not do it. `go` runs on the confirm button; a failure stays in the dialog, a success closes it.
 */
export function Confirm({ title, children, action, go, close }: { title: string; children: ReactNode; action: string; go: () => Promise<unknown>; close: () => void }) {
  const cancel = useRef<HTMLButtonElement>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  // The dialog opens first (its effect runs before this one), and Cancel takes the focus it gave.
  useEffect(() => {
    cancel.current?.focus();
  }, []);
  return (
    <Dialog labelledby="confirm-title" close={close}>
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
    </Dialog>
  );
}
