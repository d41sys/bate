import { type ReactNode, useEffect, useRef, useState } from "react";
import { SANDBOX, fileUrl } from "./frame.ts";
import { cutMiddle, placePane } from "./pane.ts";
import { X } from "lucide-react";

/** A bot's file, in the pane. */
export function FilePane({ bot, path, close }: { bot: string; path: string; close: () => void }) {
  return (
    <Pane label="Shown file" title={path} close={close}>
      <iframe title={path} sandbox={SANDBOX} src={fileUrl(bot, path)} />
      {/* Esc does not reach the page from inside the frame, but Tab leaves it: to this button, which shows only when it has the focus. */}
      <button type="button" className="leave" onClick={close}>Close <kbd>Esc</kbd></button>
    </Pane>
  );
}

/** What a bot shows or a program offers, in a pane to the right of the conversation. Resizable and closable. */
export function Pane({ label, title, close, children }: {
  label: string;
  title: string;
  close: () => void;
  children: ReactNode;
}) {
  const [want, setWant] = useState(480);
  const [viewport, setViewport] = useState(innerWidth);
  const [dragging, setDragging] = useState(false);
  const pane = useRef<HTMLElement>(null);
  useEffect(() => {
    const resize = () => setViewport(innerWidth);
    addEventListener("resize", resize);
    return () => removeEventListener("resize", resize);
  }, []);
  useEffect(() => {
    // The focus goes in, so Tab continues inside, and comes back to what opened the pane.
    const before = document.activeElement as HTMLElement | null;
    pane.current?.focus();
    return () => before?.focus();
  }, []);
  const { width, overlay } = placePane(want, viewport);
  const [start, end] = cutMiddle(title);

  return (
    <>
      {overlay && <div className="scrim" onClick={close} />}
      {/* The frame would swallow the pointer's moves while dragging over it (see style.css). */}
      <aside ref={pane} tabIndex={-1} className={`pane${overlay ? " overlay" : ""}${dragging ? " dragging" : ""}`} style={{ width }} aria-label={label}>
        <div
          className="grip"
          role="separator"
          aria-orientation="vertical"
          aria-label="Resize the pane"
          aria-valuenow={Math.round(width)}
          tabIndex={0}
          onPointerDown={(e) => {
            e.currentTarget.setPointerCapture(e.pointerId);
            setDragging(true);
          }}
          onPointerMove={(e) => dragging && setWant(innerWidth - e.clientX)}
          onPointerUp={() => setDragging(false)}
          onKeyDown={(e) => {
            if (e.key === "ArrowLeft") setWant(width + 24);
            if (e.key === "ArrowRight") setWant(width - 24);
          }}
        />
        <div className="pane-head">
          <h2 title={title}><span>{start}</span><span>{end}</span></h2>
          <button type="button" className="round ghost" aria-label="Close" data-tip="Close (Esc)" onClick={close}><X /></button>
        </div>
        {children}
      </aside>
    </>
  );
}
