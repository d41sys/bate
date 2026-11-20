import { type ReactNode, useEffect, useRef } from "react";

/**
 * The one modal: a `<dialog class="sheet">` is the scrim and its child (a form) the panel. The browser keeps the
 * focus inside and closes it on Esc; a click on the scrim closes it too. The focus goes to the first field, and
 * comes back to the control that opened the dialog. Name it with `labelledby` (a heading's id) or `label`.
 */
export function Dialog({ labelledby, label, className, close, children }: {
  labelledby?: string;
  label?: string;
  className?: string;
  close: () => void;
  children: ReactNode;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const before = document.activeElement as HTMLElement | null;
    const d = dialog.current!;
    if (!d.open) d.showModal();
    d.querySelector<HTMLElement>("input, textarea, select")?.focus();
    return () => before?.focus();
  }, []);
  return (
    <dialog
      ref={dialog}
      className={className ? `sheet ${className}` : "sheet"}
      aria-labelledby={labelledby}
      aria-label={label}
      onClose={close}
      onClick={(e) => e.target === e.currentTarget && close()}
    >
      {children}
    </dialog>
  );
}
