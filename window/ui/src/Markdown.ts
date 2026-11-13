// A reply's Markdown, with raw HTML left out: what a model writes is shown, never run.
// Plain `createElement` rather than JSX, so the test can load this file in Node as it is.
import { createElement as h } from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import rehypeHighlight from "rehype-highlight";

/** Addresses that may open, in the user's browser (the shell sends them there). */
const web = (href?: string) => (href && /^(https?:|mailto:)/i.test(href) ? href : undefined);

const components: Components = {
  a: ({ href, children }) => h("a", { href: web(href), title: web(href) }, children),
  // No image is fetched: a reply could use one to report what it saw.
  img: ({ alt }) => h("span", { className: "image" }, alt ? `[${alt}]` : "[image]"),
};

export function Markdown({ text }: { text: string }) {
  return h(
    ReactMarkdown,
    { remarkPlugins: [remarkGfm], rehypePlugins: [rehypeHighlight], skipHtml: true, components },
    text,
  );
}
