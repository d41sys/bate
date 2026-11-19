import { useEffect, useState } from "react";
import { SANDBOX, fileUrl } from "./frame.ts";
import { placePane } from "./pane.ts";

/** A file a bot shows, in a pane to the right of the conversation. Resizable and closable. */
export function Pane({ bot, path, close }: {
  bot: string;
  path: string;
  close: () => void;
}) {
  const [want, setWant] = useState(480);
  const [viewport, setViewport] = useState(innerWidth);
  // The frame would swallow the pointer's moves while dragging over it.
  const [dragging, setDragging] = useState(false);
  useEffect(() => {
    const resize = () => setViewport(innerWidth);
    addEventListener("resize", resize);
    return () => removeEventListener("resize", resize);
  }, []);
  const { width, overlay } = placePane(want, viewport);

  return (
    <aside className={overlay ? "pane overlay" : "pane"} style={{ width }} aria-label="Shown file">
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
        <code>{path}</code> <button type="button" onClick={close}>Close</button>
      </div>
      <iframe title={path} sandbox={SANDBOX} src={fileUrl(bot, path)} style={dragging ? { pointerEvents: "none" } : undefined} />
    </aside>
  );
}
