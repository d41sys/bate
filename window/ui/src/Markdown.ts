// A reply's Markdown, with raw HTML left out: what a model writes is shown, never run.
// Plain `createElement` rather than JSX, so the test can load this file in Node as it is.
import { createElement as h } from "react";
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
  return parts.length && !parts.includes("..") && !path.includes("\\") ? parts.join("/") : undefined;
}

/** `open`: shows a file the reply links to, from the bot's folder. */
export function Markdown({ text, open }: { text: string; open?: (path: string) => void }) {
  const components: Components = {
    a: ({ href, children }) => {
      const file = open && inFolder(href);
      return file
        ? h("button", { type: "button", className: "file", title: file, onClick: () => open(file) }, children)
        : h("a", { href: web(href), title: web(href) }, children);
    },
    // No image is fetched: a reply could use one to report what it saw.
    img: ({ alt }) => h("span", { className: "image" }, alt ? `[${alt}]` : "[image]"),
  };
  return h(
    ReactMarkdown,
    { remarkPlugins: [remarkGfm], rehypePlugins: [rehypeHighlight], skipHtml: true, components },
    text,
  );
}
