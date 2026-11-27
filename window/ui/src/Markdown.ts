// A reply's Markdown, with raw HTML left out: what a model writes is shown, never run.
// Plain `createElement` rather than JSX, so the test can load this file in Node as it is.
import { createElement as h, Fragment, isValidElement, useRef, useState, type ReactNode } from "react";
import { Copy } from "lucide-react";
import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import rehypeHighlight from "rehype-highlight";

/** Addresses that may open, in the user's browser (the shell sends them there). */
const web = (href?: string) => (href && /^(https?:|mailto:)/i.test(href) ? href : undefined);

/** A file in the bot's folder, by a relative path that stays inside it; `undefined` otherwise. */
export function inFolder(href?: string): string | undefined {
  if (!href || /^[a-z][a-z0-9+.-]*:/i.test(href) || /^[/#?\\]/.test(href)) return undefined;
  let path: string;
  try {
    path = decodeURIComponent(href.split(/[?#]/)[0]);
  } catch {
    return undefined;
  }
  const parts = path.split("/").filter((p) => p && p !== ".");
  // Hidden files (the engine's own) are not shown.
  return parts.length && !parts.some((p) => p.startsWith(".")) && !path.includes("\\") ? parts.join("/") : undefined;
}

/** A button that copies `text()`, and says beside it that it did. */
export function CopyButton({ text, label }: { text: () => string; label: string }) {
  const [said, setSaid] = useState("");
  // Where the clipboard is not offered (a page served over plain http), it says so: the text can still be selected.
  const copy = () =>
    Promise.resolve()
      .then(() => navigator.clipboard.writeText(text()))
      .then(() => "Copied", () => "Not copied: select the text")
      .then((now) => {
        setSaid(now);
        setTimeout(() => setSaid(""), 2000);
      });
  return h(
    Fragment,
    null,
    h("small", { role: "status" }, said),
    h("button", { type: "button", "aria-label": label, "data-tip": label, onClick: copy }, h(Copy)),
  );
}

/** A block of code: its language, when the reply named one, and Copy, at its corner. */
function Code({ children }: { children?: ReactNode }) {
  const pre = useRef<HTMLPreElement>(null);
  const lang = isValidElement<{ className?: string }>(children) ? /language-(\S+)/.exec(children.props.className ?? "")?.[1] : undefined;
  return h(
    "div",
    { className: "code" },
    h("div", { className: "tools" }, lang && h("small", null, lang), h(CopyButton, { label: "Copy the code", text: () => pre.current?.textContent ?? "" })),
    h("pre", { ref: pre }, children),
  );
}

/** `open`: shows a file the reply links to, from the bot's folder. */
export function Markdown({ text, open }: { text: string; open?: (path: string) => void }) {
  const components: Components = {
    a: ({ href, children }) => {
      const file = open && inFolder(href);
      return file
        ? h("button", { type: "button", className: "file", title: file, onClick: () => open(file) }, children)
        : h("a", { href: web(href), title: web(href), target: "_blank", rel: "noreferrer" }, children);
    },
    // No image is fetched: a reply could use one to report what it saw.
    img: ({ alt }) => h("span", { className: "image" }, alt ? `[${alt}]` : "[image]"),
    pre: ({ children }) => h(Code, null, children),
  };
  return h(
    ReactMarkdown,
    { remarkPlugins: [remarkGfm], rehypePlugins: [rehypeHighlight], skipHtml: true, components },
    text,
  );
}
