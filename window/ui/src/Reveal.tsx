import { reveal } from "./reveal.ts";

/** `text` with each character that cannot be seen shown as its code point, in a mark of its own. */
export function Revealed({ text }: { text: string }) {
  return (
    <>
      {reveal(text).map((p, i) =>
        p.hidden ? <mark key={i} className="hidden-char" title={p.hidden}>{p.text}</mark> : p.text,
      )}
    </>
  );
}
