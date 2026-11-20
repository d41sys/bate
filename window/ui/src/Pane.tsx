import { type ReactNode, useEffect, useState } from "react";
import { SANDBOX, fileUrl } from "./frame.ts";
import { placePane } from "./pane.ts";

/** A bot's file, in the pane. */
export function FilePane({ bot, path, close }: { bot: string; path: string; close: () => void }) {
  return (
    <Pane label="Shown file" title={path} close={close}>
      <iframe title={path} sandbox={SANDBOX} src={fileUrl(bot, path)} />
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
  useEffect(() => {
    const resize = () => setViewport(innerWidth);
    addEventListener("resize", resize);
    return () => removeEventListener("resize", resize);
  }, []);
  const { width, overlay } = placePane(want, viewport);

  return (
    // The frame would swallow the pointer's moves while dragging over it (see style.css).
    <aside className={`pane${overlay ? " overlay" : ""}${dragging ? " dragging" : ""}`} style={{ width }} aria-label={label}>
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
      <div className="tools">
        <code>{title}</code> <button type="button" onClick={close}>Close</button>
      </div>
      {children}
    </aside>
  );
}
