import { type ReactNode, useEffect, useRef, useState } from "react";

/** `current`: the item stands for what is shown now (a menu of pages); it is then a radio item, checked or not. */
export type MenuItem = { label: string; run: () => void; danger?: boolean; current?: boolean };

/**
 * The `⋯` button at the end of a row of a list, and the menu of the row's other actions, Delete among them.
 * Enter or Space opens it with the first item in focus; the arrows (Home, End) move, Enter runs one, Esc or a
 * click outside closes it (Esc is taken here, so it does not also stop a bot). `label` names the row.
 * With `title` the button is instead the page's title (an `<h1>` holding `title` and a `▾`), and the menu lists the pages.
 */
export function RowMenu({ label, items, title }: { label: string; items: MenuItem[]; title?: ReactNode }) {
  const [open, setOpen] = useState(false);
  const wrap = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const entries = () => [...(wrap.current?.querySelectorAll<HTMLElement>("[role^=menuitem]") ?? [])];

  useEffect(() => {
    if (!open) return;
    entries()[0]?.focus();
    const away = (e: PointerEvent) => {
      if (!wrap.current?.contains(e.target as Node)) setOpen(false);
    };
    const key = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      setOpen(false);
      trigger.current?.focus();
    };
    document.addEventListener("pointerdown", away);
    document.addEventListener("keydown", key);
    return () => {
      document.removeEventListener("pointerdown", away);
      document.removeEventListener("keydown", key);
    };
  }, [open]);

  const move = (e: React.KeyboardEvent) => {
    const all = entries();
    const at = all.indexOf(document.activeElement as HTMLElement);
    const to = { ArrowDown: (at + 1) % all.length, ArrowUp: (at - 1 + all.length) % all.length, Home: 0, End: all.length - 1 }[e.key];
    if (to === undefined) return;
    e.preventDefault();
    all[to].focus();
  };

  const button = (
    <button
      ref={trigger}
      type="button"
      className={title ? "title" : "round ghost"}
      aria-label={title ? undefined : `More actions: ${label}`}
      aria-haspopup="menu"
      aria-expanded={open}
      data-tip={title ? undefined : "More actions"}
      onClick={() => setOpen(!open)}
    >
      {title ? <>{title}<span className="chevron" aria-hidden="true">▾</span></> : <span aria-hidden="true">⋯</span>}
    </button>
  );

  return (
    <div className={title ? "menu title" : "menu more"} ref={wrap} onBlur={(e) => !wrap.current?.contains(e.relatedTarget as Node) && setOpen(false)}>
      {title ? <h1>{button}</h1> : button}
      {open && (
        <div className={title ? "popover dropdown start" : "popover dropdown"} role="menu" aria-label={label} onKeyDown={move}>
          {items.map((i) => (
            <button
              key={i.label}
              type="button"
              role={i.current === undefined ? "menuitem" : "menuitemradio"}
              aria-checked={i.current}
              tabIndex={-1}
              className={["row plain", i.danger && "danger", i.current && "current"].filter(Boolean).join(" ")}
              onClick={() => {
                setOpen(false);
                trigger.current?.focus();
                i.run();
              }}
            >
              {i.label}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
