// Serves the built page (`dist`) with the stand-in in it, on 127.0.0.1, so that any browser opens a
// named screen directly: `http://127.0.0.1:<port>/<screen>`; `/` is the page with no screen walked.
//   npm run build && npm run standin [-- <port>]
// What it shows is made-up data, not the real window.
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { dirname, extname, join, normalize, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { screens } from "./screens.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const dist = join(here, "..", "dist");
const TYPES = { ".html": "text/html", ".js": "text/javascript", ".mjs": "text/javascript", ".css": "text/css", ".svg": "image/svg+xml", ".png": "image/png", ".woff2": "font/woff2" };

/** Listens on `port` (0: a free one); resolves with the server once it listens. */
export async function serve(port = 0) {
  const page = await readFile(join(dist, "index.html"), "utf8").catch(() => {
    throw new Error("window/ui/dist is not built: run `npm run build` in window/ui first");
  });
  // First in the page, so the stand-in is there before the page's own script reads it.
  const withStandin = page.replace("<head>", '<head><script type="module" src="/standin/shell.mjs"></script>');
  const server = createServer(async (req, res) => {
    const path = decodeURIComponent(new URL(req.url, "http://x").pathname);
    const send = (status, body, type = "text/plain") => res.writeHead(status, { "content-type": type, "cache-control": "no-store" }).end(body);
    try {
      if (path === "/" || screens.some((s) => `/${s.name}` === path)) return send(200, withStandin, TYPES[".html"]);
      const root = path.startsWith("/standin/") ? here : dist;
      const file = join(root, normalize(path.replace(/^\/standin\//, "/")));
      if (!file.startsWith(root + sep)) return send(403, "no");
      send(200, await readFile(file), TYPES[extname(file)] ?? "application/octet-stream");
    } catch {
      send(404, "no such file");
    }
  });
  await new Promise((listening) => server.listen(port, "127.0.0.1", listening));
  return server;
}

if (import.meta.main) {
  const server = await serve(Number(process.argv[2] ?? 0));
  const base = `http://127.0.0.1:${server.address().port}`;
  console.log(`The built page with a stand-in for the service (made-up data). Screens:`);
  for (const s of screens) console.log(`  ${base}/${s.name}`);
}
